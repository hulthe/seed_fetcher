use seed_fetcher::{DontFetch, Resources};

// Test that the macro compiles
#[derive(Resources)]
pub struct MyState<'a> {
    #[url = "/api/test1"]
    #[allow(dead_code)]
    res: &'a String,

    #[policy = "MustBeFresh"]
    #[url = "/api/test2?foo=bar"]
    #[allow(dead_code)]
    res_2: &'a Vec<u32>,

    #[policy = "MayBeStale"]
    #[url = "/api/test34"]
    #[allow(dead_code)]
    res_3: &'a Vec<u32>,

    #[url = "/api/test34"]
    #[policy = "SilentRefetch"]
    #[allow(dead_code)]
    res_4: &'a Vec<u32>,

    #[url = "/api/test/never"]
    #[allow(dead_code)]
    res_never: DontFetch,
}

#[test]
fn test_url_getters() {
    assert_eq!(MyState::res_url(), "/api/test1");
    assert_eq!(MyState::res_2_url(), "/api/test2?foo=bar");
    assert_eq!(MyState::res_3_url(), "/api/test34");
    assert_eq!(MyState::res_4_url(), "/api/test34");
}
