use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IdError::Empty);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_id!(DocumentId);
string_id!(AnnotationId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdError {
    Empty,
}

impl fmt::Display for IdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier must not be empty")
    }
}

impl std::error::Error for IdError {}

macro_rules! generation {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }

            pub const fn next(self) -> Self {
                Self(self.0.wrapping_add(1))
            }
        }
    };
}

generation!(RenderGeneration);
generation!(SearchGeneration);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_empty_values() {
        assert_eq!(DocumentId::new(""), Err(IdError::Empty));
        assert_eq!(AnnotationId::new("  "), Err(IdError::Empty));
    }

    #[test]
    fn generations_wrap_explicitly() {
        assert_eq!(RenderGeneration::new(u64::MAX).next().get(), 0);
        assert_eq!(SearchGeneration::new(4).next().get(), 5);
    }
}
