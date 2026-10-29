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

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::json;

    #[derive(Serialize)]
    struct Hash {
        #[serde(with = "super::base64_option")]
        hash: Option<Vec<u8>>,
    }

    #[test]
    fn base64_option_is_padded_standard_base64_or_null() {
        let encode = |hash| serde_json::to_value(Hash { hash }).unwrap();
        assert_eq!(encode(Some(vec![1, 2, 3])), json!({ "hash": "AQID" }));
        assert_eq!(encode(Some(vec![0xfb, 0xff])), json!({ "hash": "+/8=" }));
        assert_eq!(encode(None), json!({ "hash": null }));
    }
}
