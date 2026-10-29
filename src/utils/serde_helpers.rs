//! Field helpers for `#[serde(with = "…")]`

/// `Option<Vec<u8>>` as an optional standard base64 string, with padding
pub mod base64_option {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => serializer.serialize_some(&STANDARD.encode(bytes)),
            None => serializer.serialize_none(),
        }
    }
}

/// With `#[serde(default)]`: an absent field is `None`, an explicit `null` is `Some(None)`
pub mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Serialize)]
    struct Hash {
        #[serde(with = "super::base64_option")]
        hash: Option<Vec<u8>>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Expiry {
        #[serde(default, with = "super::double_option")]
        expires_at: Option<Option<i64>>,
    }

    #[test]
    fn base64_option_is_padded_standard_base64_or_null() {
        let encode = |hash| serde_json::to_value(Hash { hash }).unwrap();
        assert_eq!(encode(Some(vec![1, 2, 3])), json!({ "hash": "AQID" }));
        assert_eq!(encode(Some(vec![0xfb, 0xff])), json!({ "hash": "+/8=" }));
        assert_eq!(encode(None), json!({ "hash": null }));
    }

    #[test]
    fn double_option_tells_absent_from_null() {
        let decode = |value| serde_json::from_value::<Expiry>(value).unwrap().expires_at;
        assert_eq!(decode(json!({})), None);
        assert_eq!(decode(json!({ "expires_at": null })), Some(None));
        assert_eq!(decode(json!({ "expires_at": 5 })), Some(Some(5)));
    }
}
