//! React DOM's rules for the props of host elements: which props become HTML attributes, under
//! which name, and how their values are written.
//!
//! This is `pushAttribute` from `react-dom-server` (19.3.1), as `pushStartGenericElement` calls it
//! for generic elements (`div`, `span`, `p`, void elements, …) and the `a` element. Elements with
//! rules of their own (`input`, `select`, `option`, `textarea`, `button`, `form`, `img`, `link`,
//! `meta`, `script`, `style`, `title`, `html`, `head`, `body`, custom elements) aren't covered.
//!
//! The compiler classifies props whose names it knows ([`crate::Op::Prop`]), and the engine
//! classifies the names of a props object it reads at runtime ([`crate::Op::Attributes`]), both
//! with [`classify`]. Writing the values is the engine's job.

/// How a prop's value is written as an HTML attribute. `null` and `undefined` are never written,
/// whatever the kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Strings, numbers and bigints are written; booleans are dropped (`pushStringAttribute`).
    /// `className`, `tabIndex`, `role`, and every name React doesn't know.
    String,
    /// Every value is written, booleans as `"true"` / `"false"`. `value`, `draggable`,
    /// `contentEditable`, `spellCheck`, and `data-*` / `aria-*` names.
    Any,
    /// Written as `name=""` when the value is truthy, dropped otherwise (`pushBooleanAttribute`).
    /// `hidden`, `disabled`, `async`, `autoFocus`, …
    Boolean,
    /// `true` is written as `name=""`, `false` is dropped, anything else is written (`capture`,
    /// `download`).
    OverloadedBoolean,
    /// Written when the value converts to a number that is at least 1 (`cols`, `rows`, `size`,
    /// `span`).
    PositiveNumber,
    /// Written unless the value converts to `NaN` (`rowSpan`, `start`).
    Number,
    /// A URL: booleans are dropped, and a `javascript:` URL is replaced by React's blocking URL
    /// (`action`, `formAction`, `xlinkHref`, and `href` on `a`).
    Url,
    /// A URL that is also dropped when it is `""` (`src`, and `href` on elements other than `a`).
    NonEmptyUrl,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Self; 8] = [
        Self::String,
        Self::Any,
        Self::Boolean,
        Self::OverloadedBoolean,
        Self::PositiveNumber,
        Self::Number,
        Self::Url,
        Self::NonEmptyUrl,
    ];

    /// Its byte in the program format.
    pub(crate) const fn to_byte(self) -> u8 {
        match self {
            Self::String => 0,
            Self::Any => 1,
            Self::Boolean => 2,
            Self::OverloadedBoolean => 3,
            Self::PositiveNumber => 4,
            Self::Number => 5,
            Self::Url => 6,
            Self::NonEmptyUrl => 7,
        }
    }

    /// The kind with this byte in the program format.
    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.to_byte() == byte)
    }
}

/// Which element's prop rules apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementRules {
    /// Generic elements (`pushStartGenericElement`, and `pushSelfClosing` for void elements).
    Generic,
    /// The `a` element: like generic elements, except that `href=""` is written.
    Anchor,
}

impl ElementRules {
    /// Every rule set.
    pub const ALL: [Self; 2] = [Self::Generic, Self::Anchor];

    /// Its byte in the program format.
    pub(crate) const fn to_byte(self) -> u8 {
        match self {
            Self::Generic => 0,
            Self::Anchor => 1,
        }
    }

    /// The rules with this byte in the program format.
    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.to_byte() == byte)
    }
}

/// What a prop is to React DOM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop<'a> {
    /// An HTML attribute: its name as written in HTML, and how its value is written.
    Attribute {
        /// The attribute name (`class` for `className`).
        name: &'a str,
        /// How the value is written.
        kind: Kind,
    },
    /// `style`: a CSS object, written as a `style` attribute by its own rules.
    Style,
    /// `children` or `dangerouslySetInnerHTML`: the element's content, not an attribute.
    Content,
    /// `ref`: React's Flight server throws on a host element with a ref that isn't `null` or
    /// `undefined` ("Refs cannot be used in Server Components").
    Ref,
    /// Never written: event handlers (`on…`), `defaultValue`, `defaultChecked`,
    /// `innerHTML`, the `suppress…Warning` props, and names that aren't safe in HTML.
    Ignored,
}

/// Classifies a prop by its React name, as `pushAttribute` does.
#[must_use]
pub fn classify(prop: &str, rules: ElementRules) -> Prop<'_> {
    let attribute = |name, kind| Prop::Attribute { name, kind };
    match prop {
        "children" | "dangerouslySetInnerHTML" => Prop::Content,
        "style" => Prop::Style,
        "className" => attribute("class", Kind::String),
        "tabIndex" => attribute("tabindex", Kind::String),
        "dir" | "role" | "viewBox" | "width" | "height" => attribute(prop, Kind::String),
        "href" if rules == ElementRules::Anchor => attribute(prop, Kind::Url),
        "src" | "href" => attribute(prop, Kind::NonEmptyUrl),
        "action" | "formAction" => attribute(prop, Kind::Url),
        "defaultValue"
        | "defaultChecked"
        | "innerHTML"
        | "suppressContentEditableWarning"
        | "suppressHydrationWarning" => Prop::Ignored,
        "ref" => Prop::Ref,
        "autoFocus" => attribute("autofocus", Kind::Boolean),
        "multiple" => attribute("multiple", Kind::Boolean),
        "muted" => attribute("muted", Kind::Boolean),
        "xlinkHref" => attribute("xlink:href", Kind::Url),
        "contentEditable"
        | "spellCheck"
        | "draggable"
        | "value"
        | "autoReverse"
        | "externalResourcesRequired"
        | "focusable"
        | "preserveAlpha" => attribute(prop, Kind::Any),
        "inert"
        | "allowFullScreen"
        | "async"
        | "autoPlay"
        | "controls"
        | "credentialless"
        | "default"
        | "defer"
        | "disabled"
        | "disablePictureInPicture"
        | "disableRemotePlayback"
        | "formNoValidate"
        | "hidden"
        | "loop"
        | "noModule"
        | "noValidate"
        | "open"
        | "playsInline"
        | "readOnly"
        | "required"
        | "reversed"
        | "scoped"
        | "seamless"
        | "itemScope" => attribute(prop, Kind::Boolean),
        "capture" | "download" => attribute(prop, Kind::OverloadedBoolean),
        "cols" | "rows" | "size" | "span" => attribute(prop, Kind::PositiveNumber),
        "rowSpan" | "start" => attribute(prop, Kind::Number),
        "xlinkActuate" => attribute("xlink:actuate", Kind::String),
        "xlinkArcrole" => attribute("xlink:arcrole", Kind::String),
        "xlinkRole" => attribute("xlink:role", Kind::String),
        "xlinkShow" => attribute("xlink:show", Kind::String),
        "xlinkTitle" => attribute("xlink:title", Kind::String),
        "xlinkType" => attribute("xlink:type", Kind::String),
        "xmlBase" => attribute("xml:base", Kind::String),
        "xmlLang" => attribute("xml:lang", Kind::String),
        "xmlSpace" => attribute("xml:space", Kind::String),
        _ => other(prop),
    }
}

/// The `default` branch of `pushAttribute`: event handlers are dropped, then the name is aliased
/// and checked; booleans are written only for `data-*` and `aria-*` names.
fn other(prop: &str) -> Prop<'_> {
    let bytes = prop.as_bytes();
    if bytes.len() > 2 && bytes[..2].eq_ignore_ascii_case(b"on") {
        return Prop::Ignored;
    }
    let name = alias(prop).unwrap_or(prop);
    if !is_safe_name(name) {
        return Prop::Ignored;
    }
    // `name.toLowerCase().slice(0, 5)`: no character outside ASCII lowercases to these letters.
    let prefix = name.as_bytes().get(..5);
    let kind = if prefix
        .is_some_and(|p| p.eq_ignore_ascii_case(b"data-") || p.eq_ignore_ascii_case(b"aria-"))
    {
        Kind::Any
    } else {
        Kind::String
    };
    Prop::Attribute { name, kind }
}

/// React's `aliases`: props whose HTML attribute name differs.
fn alias(prop: &str) -> Option<&'static str> {
    Some(match prop {
        "acceptCharset" => "accept-charset",
        "htmlFor" => "for",
        "httpEquiv" => "http-equiv",
        "crossOrigin" => "crossorigin",
        "accentHeight" => "accent-height",
        "alignmentBaseline" => "alignment-baseline",
        "arabicForm" => "arabic-form",
        "baselineShift" => "baseline-shift",
        "capHeight" => "cap-height",
        "clipPath" => "clip-path",
        "clipRule" => "clip-rule",
        "colorInterpolation" => "color-interpolation",
        "colorInterpolationFilters" => "color-interpolation-filters",
        "colorProfile" => "color-profile",
        "colorRendering" => "color-rendering",
        "dominantBaseline" => "dominant-baseline",
        "enableBackground" => "enable-background",
        "fillOpacity" => "fill-opacity",
        "fillRule" => "fill-rule",
        "floodColor" => "flood-color",
        "floodOpacity" => "flood-opacity",
        "fontFamily" => "font-family",
        "fontSize" => "font-size",
        "fontSizeAdjust" => "font-size-adjust",
        "fontStretch" => "font-stretch",
        "fontStyle" => "font-style",
        "fontVariant" => "font-variant",
        "fontWeight" => "font-weight",
        "glyphName" => "glyph-name",
        "glyphOrientationHorizontal" => "glyph-orientation-horizontal",
        "glyphOrientationVertical" => "glyph-orientation-vertical",
        "horizAdvX" => "horiz-adv-x",
        "horizOriginX" => "horiz-origin-x",
        "imageRendering" => "image-rendering",
        "letterSpacing" => "letter-spacing",
        "lightingColor" => "lighting-color",
        "markerEnd" => "marker-end",
        "markerMid" => "marker-mid",
        "markerStart" => "marker-start",
        "maskType" => "mask-type",
        "overlinePosition" => "overline-position",
        "overlineThickness" => "overline-thickness",
        "paintOrder" => "paint-order",
        "panose-1" => "panose-1",
        "pointerEvents" => "pointer-events",
        "renderingIntent" => "rendering-intent",
        "shapeRendering" => "shape-rendering",
        "stopColor" => "stop-color",
        "stopOpacity" => "stop-opacity",
        "strikethroughPosition" => "strikethrough-position",
        "strikethroughThickness" => "strikethrough-thickness",
        "strokeDasharray" => "stroke-dasharray",
        "strokeDashoffset" => "stroke-dashoffset",
        "strokeLinecap" => "stroke-linecap",
        "strokeLinejoin" => "stroke-linejoin",
        "strokeMiterlimit" => "stroke-miterlimit",
        "strokeOpacity" => "stroke-opacity",
        "strokeWidth" => "stroke-width",
        "textAnchor" => "text-anchor",
        "textDecoration" => "text-decoration",
        "textRendering" => "text-rendering",
        "transformOrigin" => "transform-origin",
        "underlinePosition" => "underline-position",
        "underlineThickness" => "underline-thickness",
        "unicodeBidi" => "unicode-bidi",
        "unicodeRange" => "unicode-range",
        "unitsPerEm" => "units-per-em",
        "vAlphabetic" => "v-alphabetic",
        "vHanging" => "v-hanging",
        "vIdeographic" => "v-ideographic",
        "vMathematical" => "v-mathematical",
        "vectorEffect" => "vector-effect",
        "vertAdvY" => "vert-adv-y",
        "vertOriginX" => "vert-origin-x",
        "vertOriginY" => "vert-origin-y",
        "wordSpacing" => "word-spacing",
        "writingMode" => "writing-mode",
        "xmlnsXlink" => "xmlns:xlink",
        "xHeight" => "x-height",
        _ => return None,
    })
}

/// React's `isAttributeNameSafe` (`VALID_ATTRIBUTE_NAME_REGEX`): an XML name. The regex works on
/// UTF-16 code units and its ranges end below the surrogates, so characters outside the Basic
/// Multilingual Plane are never safe.
#[must_use]
pub fn is_safe_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_name_start) && chars.all(is_name_char)
}

fn is_name_start(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}')
}

fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}
