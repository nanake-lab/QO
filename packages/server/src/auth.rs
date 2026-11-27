use std::{
    collections::HashSet,
    io,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

pub(crate) fn secret() -> io::Result<String> {
    let mut buf = [0; 32];
    getrandom::fill(&mut buf).map_err(io::Error::other)?;
    Ok(hex::encode(buf))
}

pub(crate) struct BrowserSessions {
    ttl: Duration,
    link: Option<(String, Instant)>,
    sessions: HashSet<String>,
}

impl BrowserSessions {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            link: None,
            sessions: HashSet::new(),
        }
    }

    pub fn issue(&mut self, now: Instant) -> io::Result<String> {
        let code = secret()?;
        let deadline = now
            .checked_add(self.ttl)
            .ok_or_else(|| io::Error::other("invalid login link lifetime"))?;
        self.link = Some((code.clone(), deadline));
        Ok(code)
    }

    pub fn exchange(&mut self, code: &str, now: Instant) -> io::Result<Option<String>> {
        if !self.link.as_ref().is_some_and(|(issued, until)| {
            now < *until && bool::from(issued.as_bytes().ct_eq(code.as_bytes()))
        }) {
            return Ok(None);
        }
        let cookie = secret()?;
        self.link = None;
        self.sessions.insert(cookie.clone());
        Ok(Some(cookie))
    }

    pub fn accepts(&self, cookie: &str) -> bool {
        self.sessions.contains(cookie)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn link_is_single_use_and_sessions_are_process_local() {
        let start = Instant::now();
        let ttl = Duration::from_secs(1);
        let mut auth = BrowserSessions::new(ttl);
        let stale = auth.issue(start).unwrap();
        assert!(auth.exchange(&stale, start + ttl).unwrap().is_none());
        let replaced = auth.issue(start).unwrap();
        let code = auth.issue(start).unwrap();
        assert!(auth.exchange(&replaced, start).unwrap().is_none());
        let cookie = auth.exchange(&code, start).unwrap().unwrap();
        assert!(auth.accepts(&cookie));
        assert!(auth.exchange(&code, start).unwrap().is_none());
        assert!(!BrowserSessions::new(ttl).accepts(&cookie));
    }
}
