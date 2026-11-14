//! HTTP/1.1 minimal : analyse d'une requête, réponse sérialisée. Une requête par connexion.

pub const MAX_HEAD: usize = 8 * 1024;
pub const MAX_BODY: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Pas assez d'octets : lire encore.
    Incomplete,
    /// Requête mal formée -> 400.
    Bad,
    /// Corps trop grand -> 413.
    TooLarge,
}

/// Analyse `buf` (tête + corps déjà reçus).
pub fn parse_request(buf: &[u8]) -> Result<Request, ParseError> {
    let Some(end) = find(buf, b"\r\n\r\n") else {
        return if buf.len() > MAX_HEAD {
            Err(ParseError::Bad)
        } else {
            Err(ParseError::Incomplete)
        };
    };
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| ParseError::Bad)?;
    let mut lines = head.split("\r\n");
    let rl = lines.next().ok_or(ParseError::Bad)?;
    let mut p = rl.split(' ');
    let (method, target, ver) = (p.next(), p.next(), p.next());
    let (Some(method), Some(target), Some(ver), None) = (method, target, ver, p.next()) else {
        return Err(ParseError::Bad);
    };
    if !ver.starts_with("HTTP/1.") || method.is_empty() || !target.starts_with('/') {
        return Err(ParseError::Bad);
    }
    let mut clen = 0usize;
    for l in lines {
        let (k, v) = l.split_once(':').ok_or(ParseError::Bad)?;
        if k.eq_ignore_ascii_case("content-length") {
            clen = v.trim().parse().map_err(|_| ParseError::Bad)?;
        }
    }
    if clen > MAX_BODY {
        return Err(ParseError::TooLarge);
    }
    let body = &buf[end + 4..];
    if body.len() < clen {
        return Err(ParseError::Incomplete);
    }
    let body = std::str::from_utf8(&body[..clen]).map_err(|_| ParseError::Bad)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Ok(Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        body: body.to_string(),
    })
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Response {
    pub fn json(status: u16, body: String) -> Response {
        Response {
            status,
            content_type: "application/json",
            body,
        }
    }
    pub fn error(status: u16, msg: &str) -> Response {
        Response::json(
            status,
            format!("{{\"error\":\"{}\"}}", crate::json::escape(msg)),
        )
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            _ => "Internal Server Error",
        };
        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
            self.status,
            reason,
            self.content_type,
            self.body.len(),
            self.body
        )
        .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_simple() {
        let r = parse_request(b"GET /api/state?x=1 HTTP/1.1\r\nHost: a\r\n\r\n").unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/api/state");
        assert_eq!(r.query, "x=1");
    }

    #[test]
    fn post_avec_corps() {
        let r =
            parse_request(b"POST /api/cmd HTTP/1.1\r\nContent-Length: 4\r\n\r\nabcdEXTRA").unwrap();
        assert_eq!(r.body, "abcd");
    }

    #[test]
    fn incomplet() {
        assert_eq!(
            parse_request(b"GET / HTTP/1.1\r\n"),
            Err(ParseError::Incomplete)
        );
        assert_eq!(
            parse_request(b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nab"),
            Err(ParseError::Incomplete)
        );
    }

    #[test]
    fn mal_forme() {
        for bad in [
            &b"BLABLA\r\n\r\n"[..],
            b"GET / FTP/1.0\r\n\r\n",
            b"GET nopath HTTP/1.1\r\n\r\n",
            b"GET / HTTP/1.1\r\nsans-deux-points\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n",
            b"GET / HTTP/1.1 extra\r\n\r\n",
            b"\xff\xfe / HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(parse_request(bad), Err(ParseError::Bad), "{bad:?}");
        }
    }

    #[test]
    fn corps_trop_grand() {
        let r = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(parse_request(r.as_bytes()), Err(ParseError::TooLarge));
    }

    #[test]
    fn reponse_serialisee() {
        let b = Response::error(404, "x").to_bytes();
        let s = String::from_utf8(b).unwrap();
        assert!(s.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(s.contains("Content-Length: 13\r\n"));
        assert!(s.ends_with("{\"error\":\"x\"}"));
    }
}
