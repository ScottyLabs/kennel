#[derive(Debug, thiserror::Error)]
pub enum TypeIdError {
    #[error("invalid prefix")]
    InvalidPrefix,
    #[error("invalid suffix: {0}")]
    InvalidSuffix(String),
}

macro_rules! define_typeid {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub uuid::Uuid);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let suffix: typeid_suffix::prelude::TypeIdSuffix = self.0.into();
                write!(f, "{}_{}", $prefix, suffix)
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::typeid::TypeIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let rest = s
                    .strip_prefix(concat!($prefix, "_"))
                    .ok_or($crate::typeid::TypeIdError::InvalidPrefix)?;
                let suffix: typeid_suffix::prelude::TypeIdSuffix = rest
                    .parse()
                    .map_err(|e| $crate::typeid::TypeIdError::InvalidSuffix(format!("{e}")))?;
                Ok(Self(suffix.to_uuid()))
            }
        }

        impl From<uuid::Uuid> for $name {
            fn from(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

pub(crate) use define_typeid;
