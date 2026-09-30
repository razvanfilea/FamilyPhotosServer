use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Which photos a timeline (or folder list) shows, matching Android's `PhotoType`.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PhotoFilter {
    /// Personal + Family + Shared
    #[default]
    All,
    /// Photos owned by the user
    Personal,
    /// Public photos (`user_id is null`)
    Family,
    /// Photos in someone else's folder that is shared with the user
    Shared,
}

impl PhotoFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            PhotoFilter::All => "all",
            PhotoFilter::Personal => "personal",
            PhotoFilter::Family => "family",
            PhotoFilter::Shared => "shared",
        }
    }
}

impl fmt::Display for PhotoFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PhotoFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => Ok(PhotoFilter::All),
            "personal" => Ok(PhotoFilter::Personal),
            "family" => Ok(PhotoFilter::Family),
            "shared" => Ok(PhotoFilter::Shared),
            _ => Err(format!("Unknown photo filter: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        for filter in [
            PhotoFilter::All,
            PhotoFilter::Personal,
            PhotoFilter::Family,
            PhotoFilter::Shared,
        ] {
            assert_eq!(filter.to_string().parse::<PhotoFilter>(), Ok(filter));
        }
        assert!("everything".parse::<PhotoFilter>().is_err());
        assert_eq!(PhotoFilter::default(), PhotoFilter::All);
    }
}
