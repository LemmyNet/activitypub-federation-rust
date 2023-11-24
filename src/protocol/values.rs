//! Single value enums used to receive JSON with specific expected string values
//!
//! The enums here serve to limit a json string value to a single, hardcoded value which can be
//! verified at compilation time. When using it as the type of a struct field, the struct can only
//! be constructed or deserialized if the field has the exact same value.
//!
//! In the example below, `MyObject` can only be constructed or
//! deserialized if `media_type` is `text/markdown`, but not if it is `text/html`.
//!
//! ```
//! use serde_json::from_str;
//! use serde::{Deserialize, Serialize};
//! use activitypub_federation::protocol::values::MediaTypeMarkdown;
//!
//! #[derive(Deserialize, Serialize)]
//! struct MyObject {
//!   content: String,
//!   media_type: MediaTypeMarkdown,
//! }
//!
//! let markdown_json = r#"{"content": "**test**", "media_type": "text/markdown"}"#;
//! let from_markdown = from_str::<MyObject>(markdown_json);
//! assert!(from_markdown.is_ok());
//!
//! let markdown_html = r#"{"content": "<b>test</b>", "media_type": "text/html"}"#;
//! let from_html = from_str::<MyObject>(markdown_html);
//! assert!(from_html.is_err());
//! ```
//!
//! The enums in [activitystreams_kinds] work in the same way, and can be used to
//! distinguish different activity types.

use serde::{Deserialize, Serialize};

/// Media type for markdown text.
///
/// <https://www.iana.org/assignments/media-types/media-types.xhtml>
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum MediaTypeMarkdown {
    /// `text/markdown`
    #[serde(rename = "text/markdown")]
    Markdown,
}

/// Media type for HTML text.
///
/// <https://www.iana.org/assignments/media-types/media-types.xhtml>
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum MediaTypeHtml {
    /// `text/html`
    #[serde(rename = "text/html")]
    Html,
}

/// Media type which allows both markdown and HTML.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum MediaTypeMarkdownOrHtml {
    /// `text/markdown`
    #[serde(rename = "text/markdown")]
    Markdown,
    /// `text/html`
    #[serde(rename = "text/html")]
    Html,
}
