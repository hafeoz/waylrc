use serde::Serialize;
use std::io::{self, Write};

/// A structure that can be serialized to JSON and parsed by Waybar.
#[derive(Serialize, Debug, Default)]
pub struct WaybarCustomModule {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percentage: Option<usize>,
}

impl WaybarCustomModule {
    /// Create a new module with the given contents.
    pub fn new(
        text: Option<&str>,
        alt: Option<&str>,
        tooltip: Option<&str>,
        class: Option<&str>,
        percentage: Option<usize>,
    ) -> Self {
        Self {
            text: text.map(html_escape::encode_text).map(String::from),
            alt: alt.map(html_escape::encode_text).map(String::from),
            tooltip: tooltip.map(html_escape::encode_text).map(String::from),
            class: class.map(html_escape::encode_text).map(String::from),
            percentage,
        }
    }
    /// Format the module as JSON and write it to the given writer.
    ///
    /// # Errors
    ///
    /// This function will return an error if writing to the given writer fails.
    ///
    /// # Panics
    ///
    /// This function will panic if serializing the module fails (which should never happen).
    pub fn format<T: Write>(&self, mut f: &mut T) -> io::Result<()> {
        serde_json::to_writer(&mut f, self)?;
        f.write_all(b"\n")?;
        Ok(())
    }

    /// Print the module to stdout.
    ///
    /// # Errors
    ///
    /// This function will return an error if writing to stdout fails.
    pub fn print(&self) -> io::Result<()> {
        self.format(&mut io::stdout().lock())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_format() {
        let module = WaybarCustomModule {
            text: Some("text".to_owned()),
            alt: Some("alt".to_owned()),
            tooltip: Some("tooltip".to_owned()),
            class: Some("class".to_owned()),
            percentage: Some(50),
        };
        let mut buf = Vec::new();
        module.format(&mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "{\"text\":\"text\",\"alt\":\"alt\",\"tooltip\":\"tooltip\",\"class\":\"class\",\"percentage\":50}\n"
        );
    }

    #[test]
    fn test_missing_fields() {
        let module = WaybarCustomModule {
            text: None,
            alt: None,
            tooltip: None,
            class: None,
            percentage: None,
        };
        let mut buf = Vec::new();
        module.format(&mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "{}\n");
    }
}
