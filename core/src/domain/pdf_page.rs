use std::error::Error;
use std::fmt;

/// Zero-based internal PDF page index.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageIndex(u32);

impl PageIndex {
    pub const fn new(page_index: u32) -> Self {
        Self(page_index)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for PageIndex {
    fn from(page_index: u32) -> Self {
        Self::new(page_index)
    }
}

impl From<PageIndex> for u32 {
    fn from(page_index: PageIndex) -> Self {
        page_index.get()
    }
}

impl fmt::Display for PageIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One-based PDF page number used for UI and link labels.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageNumber(u32);

impl PageNumber {
    pub const fn new(page_number: u32) -> Result<Self, PageNumberError> {
        if page_number == 0 {
            Err(PageNumberError::Zero)
        } else {
            Ok(Self(page_number))
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for PageNumber {
    type Error = PageNumberError;

    fn try_from(page_number: u32) -> Result<Self, Self::Error> {
        Self::new(page_number)
    }
}

impl From<PageNumber> for u32 {
    fn from(page_number: PageNumber) -> Self {
        page_number.get()
    }
}

impl TryFrom<PageIndex> for PageNumber {
    type Error = PageNumberError;

    fn try_from(page_index: PageIndex) -> Result<Self, Self::Error> {
        page_index
            .get()
            .checked_add(1)
            .map(Self)
            .ok_or(PageNumberError::IndexOverflow)
    }
}

impl From<PageNumber> for PageIndex {
    fn from(page_number: PageNumber) -> Self {
        Self::new(page_number.get() - 1)
    }
}

impl fmt::Display for PageNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageNumberError {
    Zero,
    IndexOverflow,
}

impl fmt::Display for PageNumberError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Zero => "page number must be at least one",
            Self::IndexOverflow => "page index has no representable one-based page number",
        };
        formatter.write_str(message)
    }
}

impl Error for PageNumberError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_page_converts_between_index_and_number() {
        let page_number = PageNumber::try_from(PageIndex::new(0)).unwrap();

        assert_eq!(page_number.get(), 1);
        assert_eq!(PageIndex::from(page_number).get(), 0);
    }

    #[test]
    fn page_number_rejects_zero_boundary() {
        assert_eq!(PageNumber::new(0), Err(PageNumberError::Zero));
        assert_eq!(PageNumber::new(1).unwrap().get(), 1);
    }

    #[test]
    fn maximum_page_number_converts_to_index() {
        let page_number = PageNumber::new(u32::MAX).unwrap();
        let page_index = PageIndex::from(page_number);

        assert_eq!(page_index.get(), u32::MAX - 1);
        assert_eq!(PageNumber::try_from(page_index), Ok(page_number));
    }

    #[test]
    fn maximum_page_index_rejects_number_overflow() {
        assert_eq!(
            PageNumber::try_from(PageIndex::new(u32::MAX)),
            Err(PageNumberError::IndexOverflow)
        );
    }

    #[test]
    fn display_uses_underlying_units() {
        assert_eq!(PageIndex::new(4).to_string(), "4");
        assert_eq!(PageNumber::new(5).unwrap().to_string(), "5");
    }
}
