use seed::app::orders::Orders;
use seed::fetch::{fetch, FetchError, Header, Request};
use seed::{error, log};
use serde::de::DeserializeOwned;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

pub use seed_fetcher_derive::Resources;

pub use self::CachePolicy::{MayBeStale, MustBeFresh, SilentRefetch};

pub type Resource = &'static str;

pub struct ResourceStore {
    cache: HashMap<Resource, CacheEntry>,
}

type DeserializeFn = Arc<dyn Fn(ContentType, &[u8]) -> Result<Box<dyn Any>, ()>>;

const ACCEPT_HEADER: &str = "application/msgpack, application/json;q=0.5";

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    DeserializeError,
    FetchError(Arc<FetchError>),
    UnsupportedContentType { content_type: String },
}

#[derive(Clone, Debug)]
pub enum ResourceMsg {
    Request(event::Request),
    Fetched(Resource, CachedResource),
    Error { resource: Resource, kind: ErrorKind },
    MarkDirty(event::MarkDirty),
}

enum CacheEntry {
    WillBeFetched,
    Fetched(CachedResource),
}

#[derive(Clone, Copy, Debug)]
enum ContentType {
    Json,
    MsgPack,
}

#[derive(Clone, Debug)]
pub struct CachedResource {
    raw: Vec<u8>,
    content_type: ContentType,
    freshness: Freshness,
    deserialized: Arc<dyn Any>,
}

#[derive(Clone, Copy, Debug)]
enum Freshness {
    Fresh,
    Dirty,
    BeingRefetched,
}

#[derive(Debug, Clone, Copy)]
pub enum NotAvailable {
    /// The resource is dirty and will be fetched again
    Stale,

    /// The resource has not been fetched yet
    NotFetched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// The dirty resource will be re-fetched before it can be acquired
    MustBeFresh,

    /// The dirty resource will _not_ trigger a re-fetch
    MayBeStale,

    /// The dirty resource can be acquired, but will also trigger a re-fetch in the background
    SilentRefetch,
}

#[derive(Copy, Clone)]
pub struct DontFetch;

pub mod event {
    use super::{DeserializeFn, ErrorKind, Resource};
    use std::fmt;

    /// A resource was requested to be fetched
    #[derive(Clone)]
    pub struct Request {
        pub resource: Resource,
        pub(super) deserialize: DeserializeFn,
    }

    /// A resource was fetched
    #[derive(Clone, Copy, Debug)]
    pub struct Fetched(pub Resource);

    /// A resource was fetched
    #[derive(Clone, Debug)]
    pub struct Error {
        pub resource: Resource,
        pub kind: ErrorKind,
    }

    /// A resource was marked as dirty
    #[derive(Clone, Copy, Debug)]
    pub struct MarkDirty(pub Resource);

    impl fmt::Debug for Request {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Request")
                .field("resource", &self.resource)
                .finish()
        }
    }
}

impl ResourceStore {
    pub fn new(orders: &mut impl Orders<ResourceMsg>) -> Self {
        orders.subscribe(ResourceMsg::Request);
        orders.subscribe(ResourceMsg::MarkDirty);
        ResourceStore {
            cache: HashMap::new(),
        }
    }

    pub fn update(&mut self, msg: ResourceMsg, orders: &mut impl Orders<ResourceMsg>) {
        match msg {
            ResourceMsg::Request(event::Request {
                resource,
                deserialize,
            }) => {
                match self.cache.get_mut(&resource) {
                    Some(CacheEntry::WillBeFetched) => return,
                    Some(CacheEntry::Fetched(CachedResource {
                        freshness: f @ Freshness::Dirty,
                        ..
                    })) => {
                        *f = Freshness::BeingRefetched;
                    }
                    Some(CacheEntry::Fetched(_)) => return,
                    None => {
                        self.cache.insert(resource, CacheEntry::WillBeFetched);
                    }
                }

                log!("resource requested", resource);
                orders.perform_cmd(async move {
                    let request = move || async move {
                        let request =
                            Request::new(resource).header(Header::custom("Accept", ACCEPT_HEADER));

                        let response = fetch(request).await?.check_status()?;

                        let bytes = response.bytes().await?;
                        let content_type = response
                            .raw_response()
                            .headers()
                            .get("content-type")
                            .expect("failed to get header");
                        Ok((bytes, content_type))
                    };
                    let response: Result<_, FetchError> = request().await;

                    match response {
                        Ok((bytes, Some(content_type))) => {
                            let content_type = match content_type.as_str() {
                                "application/json" => ContentType::Json,
                                "application/msgpack" => ContentType::MsgPack,
                                _ => {
                                    error!(
                                        &format!(
                                            "unsupported content-type {} for resource",
                                            content_type
                                        ),
                                        resource
                                    );
                                    return ResourceMsg::Error {
                                        resource,
                                        kind: ErrorKind::UnsupportedContentType { content_type },
                                    };
                                }
                            };

                            match deserialize(content_type, &bytes) {
                                Ok(deserialized) => {
                                    let cr = CachedResource {
                                        raw: bytes,
                                        content_type,
                                        freshness: Freshness::Fresh,
                                        deserialized: deserialized.into(),
                                    };
                                    ResourceMsg::Fetched(resource, cr)
                                }
                                Err(()) => {
                                    error!("failed to deserialize resource", resource);
                                    ResourceMsg::Error {
                                        resource,
                                        kind: ErrorKind::DeserializeError,
                                    }
                                }
                            }
                        }
                        Ok((_, None)) => {
                            error!(
                                "failed to deserialize resource, missing content-type",
                                resource
                            );
                            ResourceMsg::Error {
                                resource,
                                kind: ErrorKind::UnsupportedContentType {
                                    content_type: String::new(),
                                },
                            }
                        }
                        Err(fetch_error) => {
                            error!(format!("error fetching resource {}", resource), fetch_error);
                            ResourceMsg::Error {
                                resource,
                                kind: ErrorKind::FetchError(Arc::new(fetch_error)),
                            }
                        }
                    }
                });
            }
            ResourceMsg::Fetched(resource, data) => {
                log!("resource fetched", resource);
                self.cache.insert(resource, CacheEntry::Fetched(data));
                orders.notify(event::Fetched(resource));
            }
            ResourceMsg::Error { resource, kind } => {
                orders.notify(event::Error { resource, kind });
            }
            ResourceMsg::MarkDirty(event::MarkDirty(resource)) => {
                if let Some(CacheEntry::Fetched(r)) = self.cache.get_mut(&resource) {
                    r.freshness = Freshness::Dirty;
                }
            }
        }
    }

    pub fn mark_as_dirty<M: 'static>(&self, resource: Resource, orders: &mut impl Orders<M>) {
        orders.notify(event::MarkDirty(resource));
    }

    fn acquire_and_fetch<T>(
        &self,
        resource: Resource,
        policy: CachePolicy,
    ) -> (Result<&T, NotAvailable>, bool)
    where
        T: 'static + DeserializeOwned,
    {
        let result = self
            .cache
            .get(&resource)
            .ok_or((NotAvailable::NotFetched, true))
            .and_then(|e| match e {
                CacheEntry::Fetched(r) => Ok(r),
                CacheEntry::WillBeFetched => Err((NotAvailable::NotFetched, false)),
            })
            .and_then(|r| match (r.freshness, policy) {
                (Freshness::Fresh, _) | (_, CachePolicy::MayBeStale) => Ok((r, false)),
                (Freshness::Dirty, CachePolicy::SilentRefetch) => Ok((r, true)),
                (Freshness::BeingRefetched, CachePolicy::SilentRefetch) => Ok((r, false)),
                (Freshness::BeingRefetched, CachePolicy::MustBeFresh) => {
                    Err((NotAvailable::Stale, false))
                }
                (Freshness::Dirty, CachePolicy::MustBeFresh) => Err((NotAvailable::Stale, true)),
            })
            .map(|(r, fetch)| {
                let d = r
                    .deserialized
                    .downcast_ref()
                    .unwrap_or_else(|| panic!("invalid resource type for {:?}", resource));

                (d, fetch)
            });

        match result {
            Ok((r, fetch)) => (Ok(r), fetch),
            Err((e, fetch)) => (Err(e), fetch),
        }
    }

    pub fn acquire<M, T>(
        &self,
        resource: Resource,
        policy: CachePolicy,
        orders: &mut impl Orders<M>,
    ) -> Result<&T, NotAvailable>
    where
        T: 'static + DeserializeOwned,
        M: 'static,
    {
        let (r, fetch) = self.acquire_and_fetch(resource, policy);

        if fetch {
            orders.notify(event::Request {
                resource,
                deserialize: Arc::new(|content_type, bytes| {
                    let deserialized = match content_type {
                        ContentType::Json => serde_json::from_slice(&bytes).map_err(|_| ())?,
                        ContentType::MsgPack => rmp_serde::from_read_ref(&bytes).map_err(|_| ())?,
                    };
                    let v: Box<T> = Box::new(deserialized);
                    Ok(v)
                }),
            });
        }

        r
    }

    pub fn acquire_now<T>(
        &self,
        resource: Resource,
        policy: CachePolicy,
    ) -> Result<&T, NotAvailable>
    where
        T: 'static + DeserializeOwned,
    {
        self.acquire_and_fetch(resource, policy).0
    }
}
