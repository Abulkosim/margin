//! Replaces one top-level key of a JSON object in place. Every other byte of the
//! file is kept, so key order, formatting and unknown keys survive.

use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable;

struct Entry {
    key: String,
    start: usize,
    value_start: usize,
    value_end: usize,
}

struct Object {
    entries: Vec<Entry>,
    close: usize,
}

/// The raw text of a top-level key's value, if present.
pub fn raw_value<'a>(text: &'a str, key: &str) -> Result<Option<&'a str>, Unreadable> {
    let object = scan(text)?;
    Ok(object
        .entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| &text[entry.value_start..entry.value_end]))
}

/// Sets (`Some`) or removes (`None`) a top-level key. `value` is raw JSON text.
pub fn splice(text: &str, key: &str, value: Option<&str>) -> Result<String, Unreadable> {
    let original: Value = serde_json::from_str(text).map_err(|_| Unreadable)?;
    let object = scan(text)?;
    let expected: Option<Value> = value
        .map(|value| serde_json::from_str(value).map_err(|_| Unreadable))
        .transpose()?;
    let position = object.entries.iter().position(|entry| entry.key == key);
    let output = match (position, value) {
        (Some(index), Some(value)) => {
            let entry = &object.entries[index];
            format!(
                "{}{value}{}",
                &text[..entry.value_start],
                &text[entry.value_end..]
            )
        }
        (Some(index), None) => {
            let entry = &object.entries[index];
            let (from, to) = match (object.entries.get(index + 1), index.checked_sub(1)) {
                (Some(next), _) => (entry.start, next.start),
                (None, Some(previous)) => (object.entries[previous].value_end, entry.value_end),
                (None, None) => (entry.start, entry.value_end),
            };
            format!("{}{}", &text[..from], &text[to..])
        }
        (None, Some(value)) => {
            let encoded = serde_json::to_string(key).map_err(|_| Unreadable)?;
            match object.entries.last() {
                Some(last) => format!(
                    "{},\n  {encoded}: {value}{}",
                    &text[..last.value_end],
                    &text[last.value_end..]
                ),
                None => format!(
                    "{}\n  {encoded}: {value}\n{}",
                    &text[..object.close],
                    &text[object.close..]
                ),
            }
        }
        (None, None) => text.to_owned(),
    };
    let mut parsed: Value = serde_json::from_str(&output).map_err(|_| Unreadable)?;
    let mut original = original;
    let (Some(after), Some(before)) = (parsed.as_object_mut(), original.as_object_mut()) else {
        return Err(Unreadable);
    };
    if after.remove(key) != expected {
        return Err(Unreadable);
    }
    before.remove(key);
    if after != before {
        return Err(Unreadable);
    }
    Ok(output)
}

fn scan(text: &str) -> Result<Object, Unreadable> {
    let bytes = text.as_bytes();
    let mut at = skip_space(bytes, 0);
    if bytes.get(at) != Some(&b'{') {
        return Err(Unreadable);
    }
    at += 1;
    let mut entries: Vec<Entry> = Vec::new();
    loop {
        at = skip_space(bytes, at);
        match bytes.get(at) {
            Some(b'}') if entries.is_empty() => break,
            Some(b'"') => {}
            _ => return Err(Unreadable),
        }
        let start = at;
        let key_end = skip_string(bytes, at)?;
        let key: String = serde_json::from_str(&text[start..key_end]).map_err(|_| Unreadable)?;
        if entries.iter().any(|entry| entry.key == key) {
            return Err(Unreadable);
        }
        at = skip_space(bytes, key_end);
        if bytes.get(at) != Some(&b':') {
            return Err(Unreadable);
        }
        let value_start = skip_space(bytes, at + 1);
        let value_end = skip_value(bytes, value_start)?;
        entries.push(Entry {
            key,
            start,
            value_start,
            value_end,
        });
        at = skip_space(bytes, value_end);
        match bytes.get(at) {
            Some(b',') => at += 1,
            Some(b'}') => break,
            _ => return Err(Unreadable),
        }
    }
    if skip_space(bytes, at + 1) != bytes.len() {
        return Err(Unreadable);
    }
    Ok(Object { entries, close: at })
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while bytes
        .get(at)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        at += 1;
    }
    at
}

fn skip_string(bytes: &[u8], mut at: usize) -> Result<usize, Unreadable> {
    at += 1;
    while let Some(byte) = bytes.get(at) {
        match byte {
            b'\\' => at += 2,
            b'"' => return Ok(at + 1),
            _ => at += 1,
        }
    }
    Err(Unreadable)
}

fn skip_value(bytes: &[u8], at: usize) -> Result<usize, Unreadable> {
    match bytes.get(at) {
        Some(b'"') => skip_string(bytes, at),
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            let mut index = at;
            while let Some(byte) = bytes.get(index) {
                match byte {
                    b'"' => {
                        index = skip_string(bytes, index)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(index + 1);
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            Err(Unreadable)
        }
        Some(_) => {
            let mut index = at;
            while bytes.get(index).is_some_and(|byte| {
                !matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            }) {
                index += 1;
            }
            if index == at {
                Err(Unreadable)
            } else {
                Ok(index)
            }
        }
        None => Err(Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
  "numStartups": 12,
  "userID": "device",
  "oauthAccount": {
    "accountUuid": "a",
    "emailAddress": "personal@example.com"
  },
  "projects": {
    "/x": { "history": ["}\"{"] }
  },
  "tip": "a,b"
}
"#;

    #[test]
    fn replaces_only_the_key_and_keeps_every_other_byte() {
        let account =
            "{\n    \"accountUuid\": \"b\",\n    \"emailAddress\": \"work@example.com\"\n  }";
        let output = splice(CONFIG, "oauthAccount", Some(account)).unwrap();
        let before = raw_value(CONFIG, "oauthAccount").unwrap().unwrap();
        assert_eq!(output, CONFIG.replace(before, account));
        assert_eq!(raw_value(&output, "oauthAccount").unwrap(), Some(account));
        let keys: Vec<String> = scan(&output)
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert_eq!(
            keys,
            ["numStartups", "userID", "oauthAccount", "projects", "tip"]
        );
    }

    #[test]
    fn removes_and_inserts_the_key() {
        let removed = splice(CONFIG, "oauthAccount", None).unwrap();
        assert_eq!(raw_value(&removed, "oauthAccount").unwrap(), None);
        assert!(removed.contains("\"userID\": \"device\",\n  \"projects\""));
        let last = splice(
            "{\n  \"a\": 1,\n  \"oauthAccount\": {}\n}\n",
            "oauthAccount",
            None,
        )
        .unwrap();
        assert_eq!(last, "{\n  \"a\": 1\n}\n");
        let only = splice("{\"oauthAccount\":{}}", "oauthAccount", None).unwrap();
        assert_eq!(only, "{}");
        let inserted = splice("{\n  \"a\": 1\n}\n", "oauthAccount", Some("{\"x\":1}")).unwrap();
        assert_eq!(
            inserted,
            "{\n  \"a\": 1,\n  \"oauthAccount\": {\"x\":1}\n}\n"
        );
        let empty = splice("{}", "oauthAccount", Some("{}")).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&empty).unwrap(),
            serde_json::json!({"oauthAccount": {}})
        );
    }

    #[test]
    fn unreadable_or_ambiguous_files_are_refused() {
        for text in [
            "",
            "{",
            "[]",
            "{\"a\":1} x",
            "{\"a\":1,\"a\":2}",
            "{\"a\" 1}",
            "{\"a\":}",
        ] {
            assert_eq!(
                splice(text, "oauthAccount", Some("{}")),
                Err(Unreadable),
                "{text}"
            );
        }
        assert_eq!(
            splice(CONFIG, "oauthAccount", Some("{bad")),
            Err(Unreadable)
        );
    }
}
