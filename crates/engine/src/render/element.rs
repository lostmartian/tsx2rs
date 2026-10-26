//! Element nodes (ADR-0007): host elements known only at runtime, written as React DOM's server
//! renderer writes them (`pushStartInstance` and `renderElement` in react-dom-server 19.3.1) and
//! as React's Flight server serializes them (`["$",type,key,props]`).
//!
//! The props are read once, in the props object's order. HTML gets the start tag's attributes,
//! with the props some elements write out of order kept until the end of the start tag; Flight
//! gets every prop in order. The `children` entry marks where the children go in Flight, and the
//! node itself follows the other props, so the HTML content comes after the start tag. Props
//! after `children` are written to Flight after the children, from a second cursor.

use super::Renderer;
use crate::input::{Input, Primitive};
use crate::js::{self, BLOCKED_URL, is_javascript_url, is_nullish};
use crate::{RenderError, flight, html};
use tsx2rs_ir::attribute::{self, ElementRules, Prop};
use tsx2rs_ir::input::tag;

/// What React DOM's rules for an element's descendants depend on (its `FormatContext`, the parts
/// that change what this engine writes).
#[derive(Clone, Copy)]
pub(super) struct Context<'i> {
    /// The value of the enclosing `<select>` (`value`, or else `defaultValue`), which marks
    /// `<option>`s as selected. Elements that start a new context (`<svg>`, tables,
    /// `<noscript>`, …) reset it.
    select: Option<Selected<'i>>,
    /// A `<noscript>` or `<picture>` encloses the element, so React doesn't preload images.
    no_preload: bool,
}

impl<'i> Context<'i> {
    /// The document body's context: what a `Child` op's node starts in.
    pub(super) const BODY: Self = Self {
        select: None,
        no_preload: false,
    };

    /// The context of an element's children (`getChildFormatContext`).
    fn of_children(self, kind: Kind, state: &Special<'i>) -> Self {
        match kind {
            Kind::Select => Self {
                select: state.value.select().or(state.default_value.select()),
                ..self
            },
            Kind::ResetsSelect => Self {
                select: None,
                ..self
            },
            Kind::HidesImages => Self {
                select: None,
                no_preload: true,
            },
            _ => self,
        }
    }
}

/// A `<select>`'s value: one primitive, or the items of an array (`multiple`).
#[derive(Clone, Copy)]
enum Selected<'i> {
    One(Primitive<'i>),
    /// A cursor at the array's count, then its primitive items.
    Many(Input<'i>),
}

/// Which of React DOM's rules an element type gets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `pushStartGenericElement`, with these prop rules (generic, `a`, custom elements).
    Generic(ElementRules),
    /// Generic, and starts a context without a `<select>` value (`svg`, `math`, `foreignObject`,
    /// tables).
    ResetsSelect,
    /// Generic, and starts a context without a `<select>` value in which React doesn't preload
    /// images (`noscript`, `picture`).
    HidesImages,
    /// `pushSelfClosing`: `<tag …/>`, without content.
    Void,
    Img,
    Input,
    Button,
    Form,
    Select,
    Option,
    Textarea,
    /// `pre` and `listing`: a leading newline is doubled.
    Pre,
    Object,
    MenuItem,
}

impl Kind {
    /// The rules for a type, or `None` for an element this engine doesn't render yet.
    fn of(element: &str) -> Option<Self> {
        Some(match element {
            "a" => Self::Generic(ElementRules::Anchor),
            "svg" | "math" | "foreignObject" | "table" | "thead" | "tbody" | "tfoot"
            | "colgroup" | "tr" => Self::ResetsSelect,
            "noscript" | "picture" => Self::HidesImages,
            "area" | "base" | "br" | "col" | "embed" | "hr" | "keygen" | "param" | "source"
            | "track" | "wbr" => Self::Void,
            "img" => Self::Img,
            "input" => Self::Input,
            "button" => Self::Button,
            "form" => Self::Form,
            "select" => Self::Select,
            "option" => Self::Option,
            "textarea" => Self::Textarea,
            "pre" | "listing" => Self::Pre,
            "object" => Self::Object,
            "menuitem" => Self::MenuItem,
            // Hoistables and the document's own elements (ADR-0007): later pull requests.
            "title" | "meta" | "link" | "script" | "style" | "html" | "head" | "body" => {
                return None;
            }
            _ if attribute::is_custom_element(element) => Self::Generic(ElementRules::Custom),
            _ => Self::Generic(ElementRules::Generic),
        })
    }

    const fn rules(self) -> ElementRules {
        match self {
            Self::Generic(rules) => rules,
            _ => ElementRules::Generic,
        }
    }

    /// Written `<tag …/>`, without an end tag or content.
    const fn self_closing(self) -> bool {
        matches!(self, Self::Void | Self::Img | Self::Input)
    }

    /// The props written after all the others, in this order (`pushFormActionAttribute` and the
    /// `<form>` case of `pushStartInstance`).
    const fn deferred(self) -> &'static [&'static str] {
        match self {
            Self::Input | Self::Button => &[
                "name",
                "formAction",
                "formEncType",
                "formMethod",
                "formTarget",
            ],
            Self::Form => &["action", "encType", "method", "target"],
            _ => &[],
        }
    }
}

/// A prop React DOM sets aside while it writes the others, if it isn't `null` or `undefined`.
#[derive(Clone, Copy, Default)]
enum Kept<'i> {
    #[default]
    Absent,
    Primitive(Primitive<'i>),
    /// A `<select>`'s `value` or `defaultValue` that is an array.
    Items(Input<'i>),
}

impl<'i> Kept<'i> {
    const fn primitive(self) -> Option<Primitive<'i>> {
        match self {
            Self::Primitive(value) => Some(value),
            Self::Absent | Self::Items(_) => None,
        }
    }

    const fn select(self) -> Option<Selected<'i>> {
        match self {
            Self::Absent => None,
            Self::Primitive(value) => Some(Selected::One(value)),
            Self::Items(items) => Some(Selected::Many(items)),
        }
    }
}

/// The props an element type sets aside, by role.
#[derive(Default)]
struct Special<'i> {
    /// [`Kind::deferred`] props, by position.
    deferred: [Kept<'i>; 5],
    /// `value` (`input`, `select`, `option`, `textarea`).
    value: Kept<'i>,
    /// `defaultValue` (`input`, `select`, `textarea`).
    default_value: Kept<'i>,
    /// `checked` (`input`).
    checked: Kept<'i>,
    /// `defaultChecked` (`input`).
    default_checked: Kept<'i>,
    /// `selected` (`option`).
    selected: Kept<'i>,
    /// `src`, `srcSet`, `loading` and `fetchPriority` (`img`), for its preload.
    image: [Kept<'i>; 4],
    /// `dangerouslySetInnerHTML`.
    inner_html: InnerHtml<'i>,
}

/// An element's `dangerouslySetInnerHTML` prop.
#[derive(Clone, Copy, Default)]
enum InnerHtml<'i> {
    /// Absent, `null` or `undefined`.
    #[default]
    Absent,
    /// An object whose `__html` is `null` or `undefined`: nothing is written, but it still
    /// counts as set.
    Empty,
    /// An object whose `__html` is this value.
    Html(Primitive<'i>),
}

impl InnerHtml<'_> {
    const fn is_set(self) -> bool {
        !matches!(self, Self::Absent)
    }
}

impl<'i> Renderer<'_, '_> {
    /// An element node, after its tag.
    pub(super) fn element(
        &mut self,
        input: &mut Input<'i>,
        depth: usize,
        context: Context<'i>,
    ) -> Result<(), RenderError> {
        let offset = input.offset();
        let element = input.string()?;
        if !is_valid_tag(element) {
            return Err(RenderError::InvalidValue { offset }); // React: "Invalid tag"
        }
        let kind = Kind::of(element).ok_or(RenderError::UnsupportedElement { offset })?;
        self.out.flight.extend_from_slice(b"[\"$\",\"");
        self.out.flight.extend_from_slice(element.as_bytes()); // a valid tag needs no escaping
        self.out.flight.extend_from_slice(b"\",");
        let key_offset = input.offset();
        match input.u8()? {
            tag::NULL => self.out.flight.extend_from_slice(b"null"),
            tag::STRING => flight::string(&mut self.out.flight, input.string()?),
            tag => {
                return Err(RenderError::BadTag {
                    offset: key_offset,
                    tag,
                });
            }
        }
        self.out.flight.extend_from_slice(b",{");
        self.out.html.push(b'<');
        self.out.html.extend_from_slice(element.as_bytes());

        let count = input.u32()?;
        let mut state = Special::default();
        // Where `children` is: a cursor at the prop after it, and how many props follow it.
        let mut children: Option<(Input<'i>, u32)> = None;
        for i in 0..count {
            let name_offset = input.offset();
            let name = input.string()?;
            let flight = children.is_none();
            if flight && i > 0 {
                self.out.flight.push(b',');
            }
            if name == "children" {
                if children.is_some() {
                    return Err(RenderError::UnsupportedProp {
                        offset: name_offset,
                    });
                }
                flight::key(&mut self.out.flight, name);
                self.out.flight.push(b':');
                children = Some((*input, count - i - 1));
                continue;
            }
            if flight {
                flight::key(&mut self.out.flight, name);
                self.out.flight.push(b':');
            }
            self.element_prop(kind, name, input, depth, flight, &mut state)?;
        }

        // `null != children`: the node follows the props, so look at its tag.
        let has_content = match children {
            Some(_) => {
                let mut peek = *input;
                !matches!(peek.u8()?, tag::UNDEFINED | tag::NULL)
            }
            None => false,
        };
        self.end_start_tag(kind, &state, *input, context, has_content, depth, offset)?;
        self.content(kind, &state, *input, has_content, depth, offset)?;
        self.last_text = false;

        if let Some((mut rest, after)) = children {
            // The children. A textarea's are its value, written above: React doesn't render them.
            if kind == Kind::Textarea {
                self.node_flight(input, depth + 1)?;
            } else {
                self.child(input, depth + 1, context.of_children(kind, &state))?;
            }
            // The props after `children`, in Flight only.
            for _ in 0..after {
                self.out.flight.push(b',');
                flight::key(&mut self.out.flight, rest.string()?);
                self.out.flight.push(b':');
                self.value(&mut rest, depth + 1)?;
            }
        }
        self.out.flight.extend_from_slice(b"}]");
        if !kind.self_closing() {
            self.out.html.extend_from_slice(b"</");
            self.out.html.extend_from_slice(element.as_bytes());
            self.out.html.push(b'>');
        }
        self.last_text = false;
        Ok(())
    }

    /// A node in Flight only, as React's Flight server serializes it whether or not React DOM
    /// renders it.
    fn node_flight(&mut self, input: &mut Input<'i>, depth: usize) -> Result<(), RenderError> {
        let offset = input.offset();
        if depth > crate::MAX_VALUE_DEPTH {
            return Err(RenderError::TooDeep { offset });
        }
        let mut peek = *input;
        match peek.u8()? {
            tag::ARRAY => {
                *input = peek;
                self.out.flight.push(b'[');
                for i in 0..input.u32()? {
                    if i > 0 {
                        self.out.flight.push(b',');
                    }
                    self.node_flight(input, depth + 1)?;
                }
                self.out.flight.push(b']');
            }
            tag::ELEMENT => {
                *input = peek;
                self.out.flight.extend_from_slice(b"[\"$\",");
                flight::string(&mut self.out.flight, input.string()?);
                self.out.flight.push(b',');
                let key_offset = input.offset();
                match input.u8()? {
                    tag::NULL => self.out.flight.extend_from_slice(b"null"),
                    tag::STRING => flight::string(&mut self.out.flight, input.string()?),
                    tag => {
                        return Err(RenderError::BadTag {
                            offset: key_offset,
                            tag,
                        });
                    }
                }
                self.out.flight.extend_from_slice(b",{");
                let count = input.u32()?;
                let mut children: Option<(Input<'i>, u32)> = None;
                for i in 0..count {
                    let name = input.string()?;
                    if children.is_some() {
                        input.skip(depth + 1)?;
                        continue;
                    }
                    if i > 0 {
                        self.out.flight.push(b',');
                    }
                    flight::key(&mut self.out.flight, name);
                    self.out.flight.push(b':');
                    if name == "children" {
                        children = Some((*input, count - i - 1));
                    } else {
                        self.value(input, depth + 1)?;
                    }
                }
                if let Some((mut rest, after)) = children {
                    self.node_flight(input, depth + 1)?;
                    for _ in 0..after {
                        self.out.flight.push(b',');
                        flight::key(&mut self.out.flight, rest.string()?);
                        self.out.flight.push(b':');
                        self.value(&mut rest, depth + 1)?;
                    }
                }
                self.out.flight.extend_from_slice(b"}]");
            }
            tag::OBJECT => {
                return Err(RenderError::BadTag {
                    offset,
                    tag: tag::OBJECT,
                });
            }
            _ => self.value(input, depth)?,
        }
        Ok(())
    }

    /// One prop other than `children`: written to Flight if `flight`, and to HTML or set aside by
    /// the element's rules.
    fn element_prop(
        &mut self,
        kind: Kind,
        name: &'i str,
        input: &mut Input<'i>,
        depth: usize,
        flight: bool,
        state: &mut Special<'i>,
    ) -> Result<(), RenderError> {
        let offset = input.offset();
        let class = attribute::classify(name, kind.rules());
        if name == "dangerouslySetInnerHTML" {
            let html = self.inner_html(input, depth, flight)?;
            if html.is_set() && kind == Kind::Textarea {
                return Err(RenderError::InvalidValue { offset }); // "does not make sense"
            }
            state.inner_html = html;
            return Ok(());
        }
        if class == Prop::Style {
            return self.style(input, flight);
        }
        if kind == Kind::Select && matches!(name, "value" | "defaultValue") {
            let value = self.select_value(input, flight)?;
            *if name == "value" {
                &mut state.value
            } else {
                &mut state.default_value
            } = value;
            return Ok(());
        }

        let value = input.primitive()?;
        if flight {
            flight::primitive(&mut self.out.flight, value);
        }
        let kept = if is_nullish(value) {
            Kept::Absent
        } else {
            Kept::Primitive(value)
        };
        if let Some(at) = kind.deferred().iter().position(|&d| d == name) {
            state.deferred[at] = kept;
            return Ok(());
        }
        match (kind, name) {
            (Kind::Input, "checked") => state.checked = kept,
            (Kind::Input, "defaultChecked") => state.default_checked = kept,
            (Kind::Input | Kind::Textarea, "value") => state.value = kept,
            (Kind::Input | Kind::Textarea, "defaultValue") => state.default_value = kept,
            (Kind::Option, "selected") => state.selected = kept,
            (Kind::Object, "data") => self.object_data(value),
            _ => {
                match (kind, name) {
                    (Kind::Option, "value") => state.value = kept,
                    (Kind::Img, "src") => state.image[0] = kept,
                    (Kind::Img, "srcSet") => state.image[1] = kept,
                    (Kind::Img, "loading") => state.image[2] = kept,
                    (Kind::Img, "fetchPriority") => state.image[3] = kept,
                    _ => {}
                }
                self.attribute(class, value, offset)?;
            }
        }
        Ok(())
    }

    /// A prop by `pushAttribute`'s rules, classified.
    fn attribute(
        &mut self,
        class: Prop<'_>,
        value: Primitive<'_>,
        offset: usize,
    ) -> Result<(), RenderError> {
        match class {
            Prop::Attribute { name, kind } => {
                html::attribute(&mut self.out.html, name.as_bytes(), kind, value)
                    .map_err(|_| RenderError::InvalidValue { offset })
            }
            // React's Flight server throws on a ref on a host element.
            Prop::Ref if !is_nullish(value) => Err(RenderError::InvalidValue { offset }),
            Prop::Style | Prop::Content | Prop::Ref | Prop::Ignored => Ok(()),
        }
    }

    /// The rest of the start tag: the props set aside, then `>` or `/>`.
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of one element; a struct would only rename them"
    )]
    fn end_start_tag(
        &mut self,
        kind: Kind,
        state: &Special<'i>,
        children: Input<'i>,
        context: Context<'i>,
        has_content: bool,
        depth: usize,
        offset: usize,
    ) -> Result<(), RenderError> {
        for (&name, kept) in kind.deferred().iter().zip(state.deferred) {
            if let Some(value) = kept.primitive() {
                let class = attribute::classify(name, ElementRules::Generic);
                self.attribute(class, value, offset)?;
            }
        }
        match kind {
            Kind::Input => {
                if let Some(checked) = state
                    .checked
                    .primitive()
                    .or(state.default_checked.primitive())
                    && js::is_truthy(checked)
                {
                    html::empty_attribute(&mut self.out.html, b"checked");
                }
                if let Some(value) = state.value.primitive().or(state.default_value.primitive()) {
                    let class = attribute::classify("value", ElementRules::Generic);
                    self.attribute(class, value, offset)?;
                }
            }
            Kind::Option => {
                let selected = match context.select {
                    Some(select) => {
                        let mut text = Vec::new();
                        match state.value.primitive() {
                            Some(value) => js::to_string(&mut text, value),
                            None if has_content => {
                                let mut children = children;
                                option_text(&mut children, depth + 1, &mut text)?;
                            }
                            None => {}
                        }
                        is_selected(select, &text)?
                    }
                    None => state.selected.primitive().is_some_and(js::is_truthy),
                };
                if selected {
                    html::empty_attribute(&mut self.out.html, b"selected");
                }
            }
            Kind::Img if !context.no_preload && preloads(&state.image) => {
                return Err(RenderError::UnsupportedElement { offset });
            }
            _ => {}
        }
        if kind.self_closing() || kind == Kind::MenuItem {
            if has_content || state.inner_html.is_set() {
                // "… must neither have `children` nor use `dangerouslySetInnerHTML`."
                return Err(RenderError::InvalidValue { offset });
            }
        } else if has_content && state.inner_html.is_set() {
            // "Can only set one of `children` or `props.dangerouslySetInnerHTML`."
            return Err(RenderError::InvalidValue { offset });
        }
        self.out
            .html
            .extend_from_slice(if kind.self_closing() { b"/>" } else { b">" });
        Ok(())
    }

    /// Content the element writes itself, before its children: `dangerouslySetInnerHTML`, a
    /// textarea's value, a leading newline.
    fn content(
        &mut self,
        kind: Kind,
        state: &Special<'i>,
        mut children: Input<'i>,
        has_content: bool,
        depth: usize,
        offset: usize,
    ) -> Result<(), RenderError> {
        if let InnerHtml::Html(inner) = state.inner_html {
            if kind == Kind::Pre
                && let Primitive::String(s) = inner
                && s.starts_with('\n')
            {
                self.out.html.push(b'\n');
            }
            js::to_string(&mut self.out.html, inner);
        }
        match kind {
            Kind::Textarea => {
                let value = state.value.primitive().or(state.default_value.primitive());
                if has_content {
                    if value.is_some() {
                        // "If you supply `defaultValue` on a <textarea>, do not pass children."
                        return Err(RenderError::InvalidValue { offset });
                    }
                    let mut text = Vec::new();
                    textarea_text(&mut children, depth + 1, &mut text, offset)?;
                    if text.first() == Some(&b'\n') {
                        self.out.html.push(b'\n');
                    }
                    // The text is valid UTF-8: it joins whole strings and ASCII.
                    html::escape(&mut self.out.html, &String::from_utf8_lossy(&text));
                } else if let Some(value) = value {
                    if let Primitive::String(s) = value
                        && s.starts_with('\n')
                    {
                        self.out.html.push(b'\n');
                    }
                    html::value_text(&mut self.out.html, value);
                }
            }
            Kind::Pre
                if has_content
                    && children.u8()? == tag::STRING
                    && children.string()?.starts_with('\n') =>
            {
                self.out.html.push(b'\n');
            }
            _ => {}
        }
        Ok(())
    }

    /// `dangerouslySetInnerHTML`: `null`, `undefined`, or an object with an `__html` key.
    fn inner_html(
        &mut self,
        input: &mut Input<'i>,
        depth: usize,
        flight: bool,
    ) -> Result<InnerHtml<'i>, RenderError> {
        let offset = input.offset();
        match input.u8()? {
            tag::UNDEFINED => {
                if flight {
                    self.out.flight.extend_from_slice(flight::UNDEFINED);
                }
                Ok(InnerHtml::Absent)
            }
            tag::NULL => {
                if flight {
                    self.out.flight.extend_from_slice(b"null");
                }
                Ok(InnerHtml::Absent)
            }
            tag::OBJECT => {
                let mut inner = None;
                if flight {
                    self.out.flight.push(b'{');
                }
                for i in 0..input.u32()? {
                    let key = input.string()?;
                    if flight {
                        if i > 0 {
                            self.out.flight.push(b',');
                        }
                        flight::key(&mut self.out.flight, key);
                        self.out.flight.push(b':');
                    }
                    if key == "__html" {
                        let value = input.primitive()?;
                        if flight {
                            flight::primitive(&mut self.out.flight, value);
                        }
                        inner = Some(value);
                    } else if flight {
                        self.value(input, depth + 1)?;
                    } else {
                        input.skip(depth + 1)?;
                    }
                }
                if flight {
                    self.out.flight.push(b'}');
                }
                // React throws unless `"__html" in innerHTML`.
                let inner = inner.ok_or(RenderError::InvalidValue { offset })?;
                Ok(if is_nullish(inner) {
                    InnerHtml::Empty
                } else {
                    InnerHtml::Html(inner)
                })
            }
            // Not an object: React throws.
            tag::FALSE | tag::TRUE | tag::NUMBER | tag::STRING | tag::BIGINT | tag::ARRAY => {
                Err(RenderError::InvalidValue { offset })
            }
            tag => Err(RenderError::BadTag { offset, tag }),
        }
    }

    /// A `<select>`'s `value` or `defaultValue`: a primitive or an array of primitives.
    fn select_value(
        &mut self,
        input: &mut Input<'i>,
        flight: bool,
    ) -> Result<Kept<'i>, RenderError> {
        let start = *input;
        if input.u8()? != tag::ARRAY {
            *input = start;
            let value = input.primitive()?;
            if flight {
                flight::primitive(&mut self.out.flight, value);
            }
            return Ok(if is_nullish(value) {
                Kept::Absent
            } else {
                Kept::Primitive(value)
            });
        }
        let items = *input;
        if flight {
            self.out.flight.push(b'[');
        }
        for i in 0..input.u32()? {
            let value = input.primitive()?;
            if flight {
                if i > 0 {
                    self.out.flight.push(b',');
                }
                flight::primitive(&mut self.out.flight, value);
            }
        }
        if flight {
            self.out.flight.push(b']');
        }
        Ok(Kept::Items(items))
    }

    /// `<object>`'s `data`: `sanitizeURL("" + value)`, dropped when empty.
    fn object_data(&mut self, value: Primitive<'_>) {
        if is_nullish(value) {
            return;
        }
        let mut text = Vec::new();
        js::to_string(&mut text, value);
        if text.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&text); // a string, a number or ASCII: valid UTF-8
        let url = if is_javascript_url(&text) {
            BLOCKED_URL
        } else {
            &text
        };
        html::attribute_start(&mut self.out.html, b"data");
        html::escape(&mut self.out.html, url);
        self.out.html.push(b'"');
    }
}

/// React's `VALID_TAG_REGEX`: `/^[a-zA-Z][a-zA-Z:_\.\-\d]*$/`.
fn is_valid_tag(element: &str) -> bool {
    let mut bytes = element.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'-'))
}

/// Whether React would preload this `<img>` (the `img` case of `pushStartInstance`), from its
/// `src`, `srcSet`, `loading` and `fetchPriority`.
fn preloads(image: &[Kept<'_>; 4]) -> bool {
    let [src, src_set, loading, fetch_priority] = image.map(Kept::primitive);
    let is = |value: Option<Primitive<'_>>, expected: &str| matches!(value, Some(Primitive::String(s)) if s == expected);
    if is(loading, "lazy") || is(fetch_priority, "low") {
        return false;
    }
    // `src` and `srcSet` must each be a string or absent, and one of them non-empty.
    // `src` and `srcSet` must each be a string or absent (as `""` here), and one of them
    // non-empty.
    let url = |value| match value {
        None => Some(""),
        Some(Primitive::String(s)) => Some(s),
        Some(_) => None,
    };
    let (Some(src), Some(src_set)) = (url(src), url(src_set)) else {
        return false;
    };
    // A `data:` URL (any case) isn't preloaded.
    let is_data = |url: &str| {
        url.as_bytes()
            .get(..5)
            .is_some_and(|p| p[4] == b':' && p[..4].eq_ignore_ascii_case(b"data"))
    };
    (!src.is_empty() || !src_set.is_empty()) && !is_data(src) && !is_data(src_set)
}

/// Whether an option's text value is the select's value, or one of its values.
fn is_selected(select: Selected<'_>, text: &[u8]) -> Result<bool, RenderError> {
    let mut candidate = Vec::new();
    let mut matches = |value| {
        candidate.clear();
        js::to_string(&mut candidate, value);
        candidate == text
    };
    match select {
        Selected::One(value) => Ok(matches(value)),
        Selected::Many(mut items) => {
            for _ in 0..items.u32()? {
                if matches(items.primitive()?) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// An option's text (`flattenOptionChildren`): its children's strings, numbers and bigints,
/// joined; elements as `"[object Object]"`.
fn option_text(input: &mut Input<'_>, depth: usize, out: &mut Vec<u8>) -> Result<(), RenderError> {
    let offset = input.offset();
    if depth > crate::MAX_VALUE_DEPTH {
        return Err(RenderError::TooDeep { offset });
    }
    match input.u8()? {
        tag::UNDEFINED | tag::NULL | tag::FALSE | tag::TRUE => {}
        tag::NUMBER => html::number(out, input.f64()?),
        tag::STRING => out.extend_from_slice(input.string()?.as_bytes()),
        tag::BIGINT => out.extend_from_slice(input.bigint()?.as_bytes()),
        tag::ARRAY => {
            for _ in 0..input.u32()? {
                option_text(input, depth + 1, out)?;
            }
        }
        tag::ELEMENT => {
            input.skip_element(depth)?;
            out.extend_from_slice(b"[object Object]");
        }
        tag::TEMPLATE => return Err(RenderError::UnsupportedElement { offset }),
        tag => return Err(RenderError::BadTag { offset, tag }),
    }
    Ok(())
}

/// A textarea's `children` as its value: `"" + children`, which React allows for at most one
/// item in an array.
fn textarea_text(
    input: &mut Input<'_>,
    depth: usize,
    out: &mut Vec<u8>,
    element: usize,
) -> Result<(), RenderError> {
    let mut peek = *input;
    if peek.u8()? == tag::ARRAY && peek.u32()? > 1 {
        // "<textarea> can only have at most one child."
        return Err(RenderError::InvalidValue { offset: element });
    }
    node_string(input, depth, out)
}

/// JavaScript's `"" + node`: arrays joined with `,` (`null` and `undefined` items as `""`),
/// elements as `"[object Object]"`.
fn node_string(input: &mut Input<'_>, depth: usize, out: &mut Vec<u8>) -> Result<(), RenderError> {
    let offset = input.offset();
    if depth > crate::MAX_VALUE_DEPTH {
        return Err(RenderError::TooDeep { offset });
    }
    let start = *input;
    match input.u8()? {
        tag::ARRAY => {
            for i in 0..input.u32()? {
                if i > 0 {
                    out.push(b',');
                }
                let mut peek = *input;
                if matches!(peek.u8()?, tag::UNDEFINED | tag::NULL) {
                    *input = peek;
                } else {
                    node_string(input, depth + 1, out)?;
                }
            }
        }
        tag::ELEMENT => {
            input.skip_element(depth)?;
            out.extend_from_slice(b"[object Object]");
        }
        tag::TEMPLATE => return Err(RenderError::UnsupportedElement { offset }),
        _ => {
            *input = start;
            js::to_string(out, input.primitive()?);
        }
    }
    Ok(())
}
