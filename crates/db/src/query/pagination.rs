//! Pagination: offset pages and keyset cursors.
//!
//! Offset paging (`page` / `perPage`) is the PocketBase-compatible default.
//! Keyset paging (`after=<cursor>`) is the addition for large tables, where
//! `OFFSET n` gets progressively more expensive.
//!
//! The cursor is an opaque token over `(created, id)` and is only valid for the
//! default ordering; a query with a custom `sort` must use offset paging.

use lazypock_core::error::{Error, Result};

/// Page size used when the client does not ask for one.
pub const DEFAULT_PER_PAGE: u32 = 30;
/// Largest page size accepted.
pub const MAX_PER_PAGE: u32 = 500;

/// Offset pagination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    /// 1-based page number.
    pub page: u32,
    /// Rows per page.
    pub per_page: u32,
    /// Skip the `count(*)` when the client does not need `totalItems`.
    pub skip_total: bool,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: DEFAULT_PER_PAGE,
            skip_total: false,
        }
    }
}

impl PageRequest {
    /// Validate and build an offset page request.
    pub fn new(page: Option<u32>, per_page: Option<u32>, skip_total: bool) -> Result<Self> {
        let page = page.unwrap_or(1);
        if page == 0 {
            return Err(Error::invalid("page must be at least 1"));
        }
        let per_page = per_page.unwrap_or(DEFAULT_PER_PAGE);
        if per_page == 0 {
            return Err(Error::invalid("perPage must be at least 1"));
        }
        if per_page > MAX_PER_PAGE {
            return Err(Error::invalid(format!(
                "perPage must be at most {MAX_PER_PAGE}"
            )));
        }
        Ok(Self {
            page,
            per_page,
            skip_total,
        })
    }

    /// SQL `limit`.
    pub fn limit(&self) -> i64 {
        i64::from(self.per_page)
    }

    /// SQL `offset`.
    pub fn offset(&self) -> i64 {
        (i64::from(self.page) - 1) * i64::from(self.per_page)
    }

    /// Total number of pages for `total_items` rows.
    pub fn total_pages(&self, total_items: i64) -> i64 {
        let per_page = i64::from(self.per_page);
        (total_items + per_page - 1) / per_page
    }
}

/// Largest record id the cursor will carry.
const MAX_CURSOR_ID_LEN: usize = 64;

/// A keyset position over `(created, id)`.
///
/// Ids are not monotonic (they are random), which is exactly why the cursor
/// carries both `created` and `id`: `created` alone is not unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// `created` as microseconds since the Unix epoch.
    pub created_micros: i64,
    /// Tie-breaker record id.
    pub id: String,
}

impl Cursor {
    /// Build a cursor from a timestamp.
    pub fn from_time(created: std::time::SystemTime, id: impl Into<String>) -> Result<Self> {
        let micros = created
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::invalid("record timestamp predates the Unix epoch"))?
            .as_micros();
        Ok(Self {
            created_micros: micros as i64,
            id: id.into(),
        })
    }

    /// Render the opaque token.
    pub fn encode(&self) -> String {
        format!("{:x}.{}", self.created_micros, self.id)
    }

    /// Parse a token produced by [`Cursor::encode`].
    ///
    /// The id is treated as opaque: it only has to be a bounded, non-empty
    /// string. It is always bound as a parameter, never interpolated, so this is
    /// about rejecting junk rather than about safety — and it means the cursor
    /// does not break if the id format ever changes.
    pub fn decode(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Error::invalid("the pagination cursor is empty"));
        }
        let (micros, id) = raw
            .split_once('.')
            .ok_or_else(|| Error::invalid("the pagination cursor is malformed"))?;
        let created_micros = i64::from_str_radix(micros, 16)
            .map_err(|_| Error::invalid("the pagination cursor is malformed"))?;
        if id.is_empty() || id.len() > MAX_CURSOR_ID_LEN || id.chars().any(char::is_control) {
            return Err(Error::invalid(
                "the pagination cursor does not hold a valid record id",
            ));
        }
        Ok(Self {
            created_micros,
            id: id.to_string(),
        })
    }

    /// The `where` fragment selecting rows strictly after this cursor, under the
    /// default `created desc, id desc` ordering.
    ///
    /// Row comparison is used so a single predicate covers both the
    /// `created < c` and the `created = c and id < i` cases.
    pub fn predicate(&self, alias: &str, params: &mut Vec<String>) -> String {
        params.push(self.created_micros.to_string());
        let timestamp_index = params.len();
        params.push(self.id.clone());
        let id_index = params.len();
        format!(
            "({alias}.\"created\", {alias}.\"id\") < \
             (to_timestamp(${timestamp_index}::text::bigint / 1000000.0), ${id_index}::text)"
        )
    }
}

/// How a listing is paged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// `page` / `perPage`, with an optional total.
    Offset(PageRequest),
    /// Keyset paging in the default ordering.
    Keyset {
        /// Position to continue from; `None` is the first page.
        after: Option<Cursor>,
        /// Maximum rows to return.
        limit: u32,
    },
}

impl Page {
    /// Resolve the pagination from request parameters.
    ///
    /// `after` opts into keyset paging. Supplying both `page` and `after` is an
    /// error rather than a silent preference, because the two cannot be mixed.
    pub fn from_params(
        page: Option<u32>,
        per_page: Option<u32>,
        skip_total: bool,
        after: Option<&str>,
    ) -> Result<Self> {
        match after {
            Some(raw) => {
                if page.is_some() {
                    return Err(Error::invalid(
                        "`page` and `after` cannot be combined; keyset paging continues from `after`",
                    ));
                }
                let request = PageRequest::new(None, per_page, skip_total)?;
                let after = if raw.trim().is_empty() {
                    None
                } else {
                    Some(Cursor::decode(raw)?)
                };
                Ok(Self::Keyset {
                    after,
                    limit: request.per_page,
                })
            }
            None => Ok(Self::Offset(PageRequest::new(page, per_page, skip_total)?)),
        }
    }

    /// SQL `limit`.
    pub fn limit(&self) -> i64 {
        match self {
            Self::Offset(request) => request.limit(),
            Self::Keyset { limit, .. } => i64::from(*limit),
        }
    }

    /// Watermark for the next keyset page, from the last row of the current one.
    pub fn next_cursor(
        &self,
        last: Option<(std::time::SystemTime, &str)>,
    ) -> Result<Option<String>> {
        match self {
            Self::Offset(_) => Ok(None),
            Self::Keyset { .. } => match last {
                Some((created, id)) => Ok(Some(Cursor::from_time(created, id)?.encode())),
                None => Ok(None),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn page_requests_default_and_clamp() {
        let request = PageRequest::default();
        assert_eq!(request.page, 1);
        assert_eq!(request.per_page, DEFAULT_PER_PAGE);
        assert_eq!(request.limit(), 30);
        assert_eq!(request.offset(), 0);

        let request = PageRequest::new(Some(3), Some(10), true).unwrap();
        assert_eq!(request.limit(), 10);
        assert_eq!(request.offset(), 20);
        assert!(request.skip_total);
    }

    #[test]
    fn page_requests_reject_out_of_range_values() {
        for (page, per_page) in [
            (Some(0), None),
            (None, Some(0)),
            (None, Some(MAX_PER_PAGE + 1)),
            (Some(0), Some(0)),
        ] {
            assert!(
                PageRequest::new(page, per_page, false).is_err(),
                "page={page:?} per_page={per_page:?} should be rejected"
            );
        }
        assert!(PageRequest::new(None, Some(MAX_PER_PAGE), false).is_ok());
    }

    #[test]
    fn total_pages_rounds_up() {
        let request = PageRequest::new(None, Some(10), false).unwrap();
        assert_eq!(request.total_pages(0), 0);
        assert_eq!(request.total_pages(1), 1);
        assert_eq!(request.total_pages(10), 1);
        assert_eq!(request.total_pages(11), 2);
    }

    #[test]
    fn cursors_round_trip() {
        let created = UNIX_EPOCH + Duration::from_micros(1_700_000_000_123_456);
        let cursor = Cursor::from_time(created, "abcDEF123456789").unwrap();

        assert_eq!(cursor.created_micros, 1_700_000_000_123_456);
        assert_eq!(cursor.id, "abcDEF123456789");

        let encoded = cursor.encode();
        // `<micros in hex>.<id>`, with exactly one separator.
        assert_eq!(
            encoded,
            format!("{:x}.abcDEF123456789", 1_700_000_000_123_456i64)
        );
        assert_eq!(encoded.matches('.').count(), 1);

        assert_eq!(Cursor::decode(&encoded).unwrap(), cursor);
        // Whitespace around the token is tolerated.
        assert_eq!(Cursor::decode(&format!("  {encoded} ")).unwrap(), cursor);
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        for bad in [
            "",
            "   ",
            "no-separator",
            "nothex.abcDEF123456789",
            "60aea434c6840.",
        ] {
            assert!(Cursor::decode(bad).is_err(), "{bad:?} should be rejected");
        }

        // The id is opaque, but it is still bounded and free of control
        // characters so junk cannot be smuggled through.
        let too_long = format!("60aea434c6840.{}", "a".repeat(MAX_CURSOR_ID_LEN + 1));
        assert!(Cursor::decode(&too_long).is_err());
        assert!(Cursor::decode("60aea434c6840.abc\u{0007}def").is_err());

        // A short id is accepted: the cursor does not depend on the id format.
        assert_eq!(Cursor::decode("60aea434c6840.p1").unwrap().id, "p1");
    }

    #[test]
    fn keyset_predicate_carries_both_columns() {
        let cursor = Cursor {
            created_micros: 1_700_000_000_123_456,
            id: "abcDEF123456789".into(),
        };
        let mut params = Vec::new();
        let sql = cursor.predicate("p", &mut params);
        assert_eq!(
            sql,
            "(\"p\".\"created\", \"p\".\"id\") < (to_timestamp($1::text::bigint / 1000000.0), $2::text)"
                .replace("\"p\"", "p")
        );
        assert_eq!(params, vec!["1700000000123456", "abcDEF123456789"]);
    }

    #[test]
    fn from_params_chooses_offset_or_keyset() {
        assert_eq!(
            Page::from_params(None, None, false, None).unwrap(),
            Page::Offset(PageRequest::default())
        );

        // An empty `after` is the first keyset page.
        assert_eq!(
            Page::from_params(None, Some(5), false, Some("")).unwrap(),
            Page::Keyset {
                after: None,
                limit: 5
            }
        );

        let cursor = Cursor {
            created_micros: 10,
            id: "abcDEF123456789".into(),
        };
        assert_eq!(
            Page::from_params(None, None, false, Some(&cursor.encode())).unwrap(),
            Page::Keyset {
                after: Some(cursor.clone()),
                limit: DEFAULT_PER_PAGE
            }
        );

        // Mixing the two styles is an error, not a silent preference.
        let err = Page::from_params(Some(2), None, false, Some("")).unwrap_err();
        assert!(err.message().contains("cannot be combined"), "{err}");

        // A bad cursor is reported, not ignored.
        assert!(Page::from_params(None, None, false, Some("junk")).is_err());
    }

    #[test]
    fn next_cursor_is_only_produced_for_keyset_paging() {
        let created = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let offset = Page::Offset(PageRequest::default());
        assert_eq!(
            offset
                .next_cursor(Some((created, "abcDEF123456789")))
                .unwrap(),
            None
        );

        let keyset = Page::Keyset {
            after: None,
            limit: 10,
        };
        let next = keyset
            .next_cursor(Some((created, "abcDEF123456789")))
            .unwrap();
        assert_eq!(
            next,
            Some(
                Cursor::from_time(created, "abcDEF123456789")
                    .unwrap()
                    .encode()
            )
        );
        // An exhausted page has no next cursor.
        assert_eq!(keyset.next_cursor(None).unwrap(), None);
    }
}
