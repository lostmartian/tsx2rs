//! The `style` attribute, as React DOM's `pushStyleAttribute` (react-dom-server 19.3.1) writes a
//! style object: `style="name:value;name:value"`.

use crate::html::{escape, number};
use crate::input::Primitive;
use crate::js::trim;

/// Whether an entry is written: `null`, `undefined`, booleans and `""` are skipped.
pub(crate) fn is_written(value: Primitive<'_>) -> bool {
    match value {
        Primitive::Undefined | Primitive::Null | Primitive::Bool(_) => false,
        Primitive::String(s) => !s.is_empty(),
        Primitive::Number(_) | Primitive::BigInt(_) => true,
    }
}

/// Writes one written entry's `name:value` (after `style="` or `;`).
pub(crate) fn entry(out: &mut Vec<u8>, name: &str, value: Primitive<'_>) {
    if name.starts_with("--") {
        // A custom property: its name as written, and its value as text, trimmed.
        escape(out, name);
        out.push(b':');
        match value {
            Primitive::Number(n) => number(out, n),
            _ => text_value(out, value),
        }
        return;
    }
    property_name(out, name);
    out.push(b':');
    match value {
        Primitive::Number(n) => {
            number(out, n);
            if n != 0.0 && !is_unitless(name) {
                out.extend_from_slice(b"px");
            }
        }
        _ => text_value(out, value),
    }
}

/// `escapeTextForBrowser(("" + value).trim())` for strings and bigints.
fn text_value(out: &mut Vec<u8>, value: Primitive<'_>) {
    match value {
        Primitive::String(s) => escape(out, trim(s)),
        Primitive::BigInt(digits) => out.extend_from_slice(digits.as_bytes()),
        _ => {}
    }
}

/// `name.replace(/([A-Z])/g, "-$1").toLowerCase().replace(/^ms-/, "-ms-")`, escaped.
fn property_name(out: &mut Vec<u8>, name: &str) {
    let hyphenated: String;
    let lower: String;
    let name = if name.is_ascii() {
        hyphenated = name
            .chars()
            .flat_map(|c| {
                let dash = c.is_ascii_uppercase().then_some('-');
                dash.into_iter()
                    .chain(std::iter::once(c.to_ascii_lowercase()))
            })
            .collect();
        hyphenated.as_str()
    } else {
        // `toLowerCase` on the whole string: final sigma depends on the letters around it.
        hyphenated = name
            .chars()
            .flat_map(|c| {
                let dash = c.is_ascii_uppercase().then_some('-');
                dash.into_iter().chain(std::iter::once(c))
            })
            .collect();
        lower = hyphenated.to_lowercase();
        lower.as_str()
    };
    if name.starts_with("ms-") {
        out.push(b'-');
    }
    escape(out, name);
}

/// React's `unitlessNumbers`: properties whose numbers get no `px`.
fn is_unitless(name: &str) -> bool {
    matches!(
        name,
        "animationIterationCount"
            | "aspectRatio"
            | "borderImageOutset"
            | "borderImageSlice"
            | "borderImageWidth"
            | "boxFlex"
            | "boxFlexGroup"
            | "boxOrdinalGroup"
            | "columnCount"
            | "columns"
            | "flex"
            | "flexGrow"
            | "flexPositive"
            | "flexShrink"
            | "flexNegative"
            | "flexOrder"
            | "gridArea"
            | "gridRow"
            | "gridRowEnd"
            | "gridRowSpan"
            | "gridRowStart"
            | "gridColumn"
            | "gridColumnEnd"
            | "gridColumnSpan"
            | "gridColumnStart"
            | "fontWeight"
            | "lineClamp"
            | "lineHeight"
            | "opacity"
            | "order"
            | "orphans"
            | "scale"
            | "tabSize"
            | "widows"
            | "zIndex"
            | "zoom"
            | "fillOpacity"
            | "floodOpacity"
            | "stopOpacity"
            | "strokeDasharray"
            | "strokeDashoffset"
            | "strokeMiterlimit"
            | "strokeOpacity"
            | "strokeWidth"
            | "MozAnimationIterationCount"
            | "MozBoxFlex"
            | "MozBoxFlexGroup"
            | "MozLineClamp"
            | "msAnimationIterationCount"
            | "msFlex"
            | "msZoom"
            | "msFlexGrow"
            | "msFlexNegative"
            | "msFlexOrder"
            | "msFlexPositive"
            | "msFlexShrink"
            | "msGridColumn"
            | "msGridColumnSpan"
            | "msGridRow"
            | "msGridRowSpan"
            | "WebkitAnimationIterationCount"
            | "WebkitBoxFlex"
            | "WebKitBoxFlexGroup"
            | "WebkitBoxOrdinalGroup"
            | "WebkitColumnCount"
            | "WebkitColumns"
            | "WebkitFlex"
            | "WebkitFlexGrow"
            | "WebkitFlexPositive"
            | "WebkitFlexShrink"
            | "WebkitLineClamp"
    )
}

#[cfg(test)]
mod tests {
    use super::{entry, property_name};
    use crate::input::Primitive;

    fn name(s: &str) -> String {
        let mut out = Vec::new();
        property_name(&mut out, s);
        String::from_utf8(out).unwrap()
    }

    fn written(n: &str, value: Primitive<'_>) -> String {
        let mut out = Vec::new();
        entry(&mut out, n, value);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn hyphenates_property_names_like_react() {
        let cases = [
            ("color", "color"),
            ("backgroundColor", "background-color"),
            ("WebkitTransition", "-webkit-transition"),
            ("MozBoxFlex", "-moz-box-flex"),
            ("msTransform", "-ms-transform"),
            ("MsTransform", "-ms-transform"),
            ("ms", "ms"),
            ("XMLHttp", "-x-m-l-http"),
            ("a\"b<", "a&quot;b&lt;"),
            ("ÄpfelGröße", "äpfel-größe"),
            ("ΟΔΟΣ", "\u{3bf}\u{3b4}\u{3bf}\u{3c2}"), // Unicode lowercase, with a final sigma
            ("İ", "i\u{307}"),
        ];
        for (input, expected) in cases {
            assert_eq!(name(input), expected, "{input:?}");
        }
    }

    #[test]
    fn writes_values_like_react() {
        assert_eq!(written("width", Primitive::Number(10.0)), "width:10px");
        assert_eq!(written("width", Primitive::Number(0.0)), "width:0");
        assert_eq!(written("width", Primitive::Number(-0.0)), "width:0");
        assert_eq!(written("width", Primitive::Number(f64::NAN)), "width:NaNpx");
        assert_eq!(written("opacity", Primitive::Number(0.5)), "opacity:0.5");
        assert_eq!(
            written("WebKitBoxFlexGroup", Primitive::Number(2.0)),
            "-web-kit-box-flex-group:2"
        );
        assert_eq!(written("width", Primitive::String(" 1em\n")), "width:1em");
        assert_eq!(written("width", Primitive::String("  ")), "width:");
        assert_eq!(
            written("content", Primitive::String("'a'")),
            "content:&#x27;a&#x27;"
        );
        assert_eq!(written("width", Primitive::BigInt("12")), "width:12");
        assert_eq!(written("--gap", Primitive::Number(4.0)), "--gap:4");
        assert_eq!(
            written("--myColor", Primitive::String(" red ")),
            "--myColor:red"
        );
    }
}
