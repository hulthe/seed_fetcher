use seed_fetcher::Resources;

pub struct NotAvailable;
pub struct MustBeFresh;

pub trait Orders<T> {}

pub struct ResourceStore;

impl ResourceStore {
    pub fn acquire<T, M>(
        &self,
        _: &'static str,
        _: MustBeFresh,
        _: &mut impl Orders<M>,
    ) -> Result<T, NotAvailable> {
        unimplemented!()
    }

    pub fn acquire_now<T>(&self, _: &'static str, _: MustBeFresh) -> Result<T, NotAvailable> {
        unimplemented!()
    }
}

// Test that the macro compiles
#[derive(Resources)]
pub struct MyState<'a> {
    #[url = "/api/test1"]
    #[allow(dead_code)]
    res: &'a String,

    #[url = "/api/test2?foo=bar"]
    #[allow(dead_code)]
    res_2: &'a Vec<u32>,
}

#[test]
fn test_url_getters() {
    assert_eq!(MyState::res_url(), "/api/test1");
    assert_eq!(MyState::res_2_url(), "/api/test2?foo=bar");
}
