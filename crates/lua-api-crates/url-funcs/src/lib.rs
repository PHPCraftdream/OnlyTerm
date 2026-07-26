#[derive(Clone, Debug)]
pub struct Url {
    pub url: url::Url,
}

impl std::ops::Deref for Url {
    type Target = url::Url;
    fn deref(&self) -> &url::Url {
        &self.url
    }
}

impl std::ops::DerefMut for Url {
    fn deref_mut(&mut self) -> &mut url::Url {
        &mut self.url
    }
}
