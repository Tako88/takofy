//! Test doubles shared by the discovery tests.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};

use super::{Fetch, FetchError};

/// A fetch that answers scripted bodies and counts every call.
#[derive(Clone, Default)]
pub(crate) struct FakeFetch {
    inner: Arc<Inner>,
}

type Answer = Result<Vec<u8>, FetchError>;
type Routes = Mutex<Vec<(String, VecDeque<Answer>)>>;

#[derive(Default)]
struct Inner {
    routes: Routes,
    calls: Mutex<Vec<String>>,
}

impl FakeFetch {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&self, needle: &str, response: Result<Vec<u8>, FetchError>) {
        let mut routes = self.inner.routes.lock().expect("routes");
        match routes.iter_mut().find(|(name, _)| name == needle) {
            Some((_, queue)) => queue.push_back(response),
            None => routes.push((needle.to_owned(), VecDeque::from([response]))),
        }
    }

    pub(crate) fn ok(&self, needle: &str, body: Vec<u8>) {
        self.push(needle, Ok(body));
    }

    pub(crate) fn calls(&self) -> Vec<String> {
        self.inner.calls.lock().expect("calls").clone()
    }

    pub(crate) fn count(&self) -> usize {
        self.inner.calls.lock().expect("calls").len()
    }

    fn response_for(&self, path: &str) -> Result<Vec<u8>, FetchError> {
        let mut routes = self.inner.routes.lock().expect("routes");
        let Some((_, queue)) = routes
            .iter_mut()
            .find(|(needle, _)| path.contains(needle.as_str()))
        else {
            return Err(FetchError::Transport);
        };
        queue.pop_front().unwrap_or(Err(FetchError::Transport))
    }
}

impl Fetch for FakeFetch {
    fn get(
        &self,
        path_and_query: &str,
    ) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send {
        self.inner
            .calls
            .lock()
            .expect("calls")
            .push(path_and_query.to_owned());
        let response = self.response_for(path_and_query);
        async move { response }
    }
}
