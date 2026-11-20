//! `-created,title` → ORDER BY, with every field checked against the schema.

use crate::error::{self};
use crate::schema::Collection;
use crate::types::Kind;
use sea_query::{Alias, Order};

use lazypock_core::Result;

const MAX_KEYS: usize = 4;

/// Returns `(column, direction)` pairs. Always ends with `id` so pagination is
/// stable when the requested keys tie.
pub fn parse(c: &Collection, spec: Option<&str>) -> Result<Vec<(Alias, Order)>> {
    let bad = |m: String| error::invalid_sort(m);
    let mut out: Vec<(Alias, Order)> = Vec::new();
    let mut has_id = false;

    for raw in spec
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if out.len() >= MAX_KEYS {
            return Err(bad(format!("at most {MAX_KEYS} sort keys")));
        }
        let (name, order) = match raw.strip_prefix('-') {
            Some(n) => (n, Order::Desc),
            None => (raw.strip_prefix('+').unwrap_or(raw), Order::Asc),
        };
        match c.column_kind(name) {
            None => return Err(bad(format!("unknown field {name:?}"))),
            Some(Kind::Json) => return Err(bad(format!("cannot sort by json field {name:?}"))),
            Some(_) => {}
        }
        has_id |= name == "id";
        out.push((Alias::new(name), order));
    }
    if !has_id {
        out.push((Alias::new("id"), Order::Asc));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Field;
    use crate::types::{CollectionType, FieldType};

    fn coll() -> Collection {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        c.fields = vec![
            Field::new("title", FieldType::Text).unwrap(),
            Field::new("meta", FieldType::Json).unwrap(),
        ];
        c
    }

    fn names(v: &[(Alias, Order)]) -> Vec<String> {
        v.iter()
            .map(|(a, o)| {
                format!(
                    "{}{}",
                    if matches!(o, Order::Desc) { "-" } else { "" },
                    sea_query::Iden::to_string(a)
                )
            })
            .collect()
    }

    #[test]
    fn parses_directions_and_appends_id() {
        assert_eq!(
            names(&parse(&coll(), Some("-created, title")).unwrap()),
            ["-created", "title", "id"]
        );
        assert_eq!(names(&parse(&coll(), None).unwrap()), ["id"]);
        assert_eq!(names(&parse(&coll(), Some("-id")).unwrap()), ["-id"]);
    }

    #[test]
    fn rejects_unknown_json_and_too_many() {
        assert!(parse(&coll(), Some("nope")).is_err());
        assert!(parse(&coll(), Some("meta")).is_err());
        assert!(parse(&coll(), Some("title; DROP TABLE x")).is_err());
        assert!(parse(&coll(), Some("id,id,id,id,id")).is_err());
    }
}
