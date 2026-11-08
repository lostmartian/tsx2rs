//! Component elements in a module's JSX: `<Gallery images={…} />`, `<Link href="/">…</Link>`.
//!
//! Only elements whose tag is a component (a capitalized identifier or a member expression, as
//! React decides) are collected; host elements (`<div>`) are not. Each use records its props as
//! written: a literal (with its exact JSON, so it can become a static Flight chunk), an expression
//! (read at runtime), or a spread.

use oxc_ast::ast::{
    Expression, JSXAttributeItem, JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement,
    JSXElementName, JSXExpression, JSXMemberExpressionObject, Program, UnaryOperator,
};
use oxc_ast_visit::{Visit, walk};

/// One component element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentUse {
    /// The local binding the tag refers to (`Link`, or `ui` in `<ui.Button>`).
    pub name: String,
    /// For `<ui.Button>`: `Button` (nested members joined with `.`).
    pub member: Option<String>,
    /// Byte offsets of the opening tag.
    pub span: (u32, u32),
    /// Props in source order.
    pub props: Vec<Prop>,
    /// Whether the element has children (after JSX whitespace rules).
    pub has_children: bool,
}

/// A prop as written on the element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    /// The prop name (`className`, `aria-label`); empty for a spread.
    pub name: String,
    /// Its value.
    pub value: PropValue,
}

/// A prop's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropValue {
    /// A literal: its JSON (`"relative h-full"`, `true`, `42`, `null`).
    Literal(String),
    /// Any other expression: its source text.
    Expression(String),
    /// `{...props}`: the spread expression's source text.
    Spread(String),
}

/// Collects the component elements of a module, in source order.
#[must_use]
pub fn collect(program: &Program<'_>) -> Vec<ComponentUse> {
    let mut collector = Collector {
        source: program.source_text,
        uses: Vec::new(),
    };
    collector.visit_program(program);
    collector.uses
}

struct Collector<'s> {
    source: &'s str,
    uses: Vec<ComponentUse>,
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
        let opening = &element.opening_element;
        let tag = match &opening.name {
            JSXElementName::IdentifierReference(id) => Some((id.name.as_str().to_owned(), None)),
            JSXElementName::MemberExpression(member) => {
                let mut path = vec![member.property.name.as_str().to_owned()];
                let mut object = &member.object;
                loop {
                    match object {
                        JSXMemberExpressionObject::MemberExpression(inner) => {
                            path.push(inner.property.name.as_str().to_owned());
                            object = &inner.object;
                        }
                        JSXMemberExpressionObject::IdentifierReference(id) => {
                            path.reverse();
                            break Some((id.name.as_str().to_owned(), Some(path.join("."))));
                        }
                        JSXMemberExpressionObject::ThisExpression(_) => break None,
                    }
                }
            }
            JSXElementName::Identifier(_)
            | JSXElementName::NamespacedName(_)
            | JSXElementName::ThisExpression(_) => None,
        };
        if let Some((name, member)) = tag {
            let props = opening.attributes.iter().map(|a| self.prop(a)).collect();
            self.uses.push(ComponentUse {
                name,
                member,
                span: (opening.span.start, opening.span.end),
                props,
                has_children: element.children.iter().any(is_child),
            });
        }
        walk::walk_jsx_element(self, element);
    }
}

impl Collector<'_> {
    fn text(&self, span: oxc_span::Span) -> String {
        self.source
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default()
            .to_owned()
    }

    fn prop(&self, item: &JSXAttributeItem<'_>) -> Prop {
        match item {
            JSXAttributeItem::SpreadAttribute(spread) => Prop {
                name: String::new(),
                value: PropValue::Spread(self.text(oxc_span::GetSpan::span(&spread.argument))),
            },
            JSXAttributeItem::Attribute(attribute) => {
                let name = match &attribute.name {
                    JSXAttributeName::Identifier(id) => id.name.as_str().to_owned(),
                    JSXAttributeName::NamespacedName(n) => {
                        format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str())
                    }
                };
                let value = match &attribute.value {
                    None => PropValue::Literal("true".into()), // <Link prefetch />
                    Some(JSXAttributeValue::StringLiteral(s)) => match s.value.as_str() {
                        // A lone surrogate can't be a static literal: read it at runtime instead.
                        Some(value) => PropValue::Literal(json_string(&decode_entities(value))),
                        None => PropValue::Expression(self.text(s.span)),
                    },
                    Some(JSXAttributeValue::ExpressionContainer(container)) => match &container
                        .expression
                    {
                        JSXExpression::EmptyExpression(_) => PropValue::Expression(String::new()),
                        expression => literal(expression).map_or_else(
                            || {
                                PropValue::Expression(
                                    self.text(oxc_span::GetSpan::span(expression)),
                                )
                            },
                            PropValue::Literal,
                        ),
                    },
                    Some(other) => PropValue::Expression(self.text(oxc_span::GetSpan::span(other))),
                };
                Prop { name, value }
            }
        }
    }
}

/// The JSON of a literal expression, or `None` if the expression isn't a literal.
fn literal(expression: &JSXExpression<'_>) -> Option<String> {
    match expression {
        JSXExpression::StringLiteral(s) => s.value.as_str().map(json_string),
        JSXExpression::NumericLiteral(n) => json_number(n.value),
        JSXExpression::BooleanLiteral(b) => Some(b.value.to_string()),
        JSXExpression::NullLiteral(_) => Some("null".into()),
        JSXExpression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked?.as_str())
            .map(json_string),
        JSXExpression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match &u.argument {
                Expression::NumericLiteral(n) => json_number(-n.value),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A JSON number as `JSON.stringify` writes it, or `None` for values JSON can't hold.
pub(crate) fn json_number(n: f64) -> Option<String> {
    if !n.is_finite() {
        return None;
    }
    if n == 0.0 {
        return Some("0".into()); // JSON.stringify(-0) is "0"
    }
    Some(ryu_js::Buffer::new().format_finite(n).to_owned())
}

/// A JSON string literal (as `JSON.stringify` writes it).
pub(crate) fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether a JSX child produces a child (React drops whitespace-only text that spans lines, and
/// empty `{}` / `{/* comments */}` containers).
fn is_child(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => jsx_text(text.value.as_str()).is_some(),
        JSXChild::ExpressionContainer(c) => {
            !matches!(c.expression, JSXExpression::EmptyExpression(_))
        }
        JSXChild::Element(_) | JSXChild::Fragment(_) | JSXChild::Spread(_) => true,
    }
}

/// JSX text as React sees it: lines trimmed (except the start of the first and the end of the
/// last), whitespace-only lines dropped, the rest joined with single spaces. `None` if nothing is
/// left. This is the rule Babel, TypeScript and oxc's JSX transform apply.
#[must_use]
pub fn jsx_text(raw: &str) -> Option<String> {
    let raw = &*decode_entities(raw);
    let lines: Vec<&str> = raw.split(['\n', '\r']).collect();
    let last = lines.len().saturating_sub(1);
    let mut parts = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut line: &str = line;
        if i > 0 {
            line = line.trim_start_matches([' ', '\t']);
        }
        if i < last {
            line = line.trim_end_matches([' ', '\t']);
        }
        if !line.is_empty() {
            parts.push(line);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// The named character references JSX decodes (XHTML's 253, as Babel's and SWC's JSX lexers
/// have them), sorted by name: name and code point.
const ENTITIES: &[(&str, u32)] = &[
    ("AElig", 198),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Agrave", 192),
    ("Alpha", 913),
    ("Aring", 197),
    ("Atilde", 195),
    ("Auml", 196),
    ("Beta", 914),
    ("Ccedil", 199),
    ("Chi", 935),
    ("Dagger", 8225),
    ("Delta", 916),
    ("ETH", 208),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Egrave", 200),
    ("Epsilon", 917),
    ("Eta", 919),
    ("Euml", 203),
    ("Gamma", 915),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Igrave", 204),
    ("Iota", 921),
    ("Iuml", 207),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Ntilde", 209),
    ("Nu", 925),
    ("OElig", 338),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Ograve", 210),
    ("Omega", 937),
    ("Omicron", 927),
    ("Oslash", 216),
    ("Otilde", 213),
    ("Ouml", 214),
    ("Phi", 934),
    ("Pi", 928),
    ("Prime", 8243),
    ("Psi", 936),
    ("Rho", 929),
    ("Scaron", 352),
    ("Sigma", 931),
    ("THORN", 222),
    ("Tau", 932),
    ("Theta", 920),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Ugrave", 217),
    ("Upsilon", 933),
    ("Uuml", 220),
    ("Xi", 926),
    ("Yacute", 221),
    ("Yuml", 376),
    ("Zeta", 918),
    ("aacute", 225),
    ("acirc", 226),
    ("acute", 180),
    ("aelig", 230),
    ("agrave", 224),
    ("alefsym", 8501),
    ("alpha", 945),
    ("amp", 38),
    ("and", 8743),
    ("ang", 8736),
    ("apos", 39),
    ("aring", 229),
    ("asymp", 8776),
    ("atilde", 227),
    ("auml", 228),
    ("bdquo", 8222),
    ("beta", 946),
    ("brvbar", 166),
    ("bull", 8226),
    ("cap", 8745),
    ("ccedil", 231),
    ("cedil", 184),
    ("cent", 162),
    ("chi", 967),
    ("circ", 710),
    ("clubs", 9827),
    ("cong", 8773),
    ("copy", 169),
    ("crarr", 8629),
    ("cup", 8746),
    ("curren", 164),
    ("dArr", 8659),
    ("dagger", 8224),
    ("darr", 8595),
    ("deg", 176),
    ("delta", 948),
    ("diams", 9830),
    ("divide", 247),
    ("eacute", 233),
    ("ecirc", 234),
    ("egrave", 232),
    ("empty", 8709),
    ("emsp", 8195),
    ("ensp", 8194),
    ("epsilon", 949),
    ("equiv", 8801),
    ("eta", 951),
    ("eth", 240),
    ("euml", 235),
    ("euro", 8364),
    ("exist", 8707),
    ("fnof", 402),
    ("forall", 8704),
    ("frac12", 189),
    ("frac14", 188),
    ("frac34", 190),
    ("frasl", 8260),
    ("gamma", 947),
    ("ge", 8805),
    ("gt", 62),
    ("hArr", 8660),
    ("harr", 8596),
    ("hearts", 9829),
    ("hellip", 8230),
    ("iacute", 237),
    ("icirc", 238),
    ("iexcl", 161),
    ("igrave", 236),
    ("image", 8465),
    ("infin", 8734),
    ("int", 8747),
    ("iota", 953),
    ("iquest", 191),
    ("isin", 8712),
    ("iuml", 239),
    ("kappa", 954),
    ("lArr", 8656),
    ("lambda", 955),
    ("lang", 9001),
    ("laquo", 171),
    ("larr", 8592),
    ("lceil", 8968),
    ("ldquo", 8220),
    ("le", 8804),
    ("lfloor", 8970),
    ("lowast", 8727),
    ("loz", 9674),
    ("lrm", 8206),
    ("lsaquo", 8249),
    ("lsquo", 8216),
    ("lt", 60),
    ("macr", 175),
    ("mdash", 8212),
    ("micro", 181),
    ("middot", 183),
    ("minus", 8722),
    ("mu", 956),
    ("nabla", 8711),
    ("nbsp", 160),
    ("ndash", 8211),
    ("ne", 8800),
    ("ni", 8715),
    ("not", 172),
    ("notin", 8713),
    ("nsub", 8836),
    ("ntilde", 241),
    ("nu", 957),
    ("oacute", 243),
    ("ocirc", 244),
    ("oelig", 339),
    ("ograve", 242),
    ("oline", 8254),
    ("omega", 969),
    ("omicron", 959),
    ("oplus", 8853),
    ("or", 8744),
    ("ordf", 170),
    ("ordm", 186),
    ("oslash", 248),
    ("otilde", 245),
    ("otimes", 8855),
    ("ouml", 246),
    ("para", 182),
    ("part", 8706),
    ("permil", 8240),
    ("perp", 8869),
    ("phi", 966),
    ("pi", 960),
    ("piv", 982),
    ("plusmn", 177),
    ("pound", 163),
    ("prime", 8242),
    ("prod", 8719),
    ("prop", 8733),
    ("psi", 968),
    ("quot", 34),
    ("rArr", 8658),
    ("radic", 8730),
    ("rang", 9002),
    ("raquo", 187),
    ("rarr", 8594),
    ("rceil", 8969),
    ("rdquo", 8221),
    ("real", 8476),
    ("reg", 174),
    ("rfloor", 8971),
    ("rho", 961),
    ("rlm", 8207),
    ("rsaquo", 8250),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("scaron", 353),
    ("sdot", 8901),
    ("sect", 167),
    ("shy", 173),
    ("sigma", 963),
    ("sigmaf", 962),
    ("sim", 8764),
    ("spades", 9824),
    ("sub", 8834),
    ("sube", 8838),
    ("sum", 8721),
    ("sup", 8835),
    ("sup1", 185),
    ("sup2", 178),
    ("sup3", 179),
    ("supe", 8839),
    ("szlig", 223),
    ("tau", 964),
    ("there4", 8756),
    ("theta", 952),
    ("thetasym", 977),
    ("thinsp", 8201),
    ("thorn", 254),
    ("tilde", 732),
    ("times", 215),
    ("trade", 8482),
    ("uArr", 8657),
    ("uacute", 250),
    ("uarr", 8593),
    ("ucirc", 251),
    ("ugrave", 249),
    ("uml", 168),
    ("upsih", 978),
    ("upsilon", 965),
    ("uuml", 252),
    ("weierp", 8472),
    ("xi", 958),
    ("yacute", 253),
    ("yen", 165),
    ("yuml", 255),
    ("zeta", 950),
    ("zwj", 8205),
    ("zwnj", 8204),
];

/// JSX text or a JSX attribute's string with its character references decoded, as the JSX
/// lexer does before anything else: `&name;` from XHTML's names (at most 10 characters before the
/// `;`), `&#123;` and `&#x7b;`; anything else is a literal `&`.
#[must_use]
pub fn decode_entities(raw: &str) -> std::borrow::Cow<'_, str> {
    if !raw.contains('&') {
        return std::borrow::Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let decoded = rest.find(';').and_then(|semi| {
            let body = &rest[..semi];
            let c = if let Some(number) = body.strip_prefix('#') {
                let (digits, radix) = match number.strip_prefix('x') {
                    Some(hex) => (hex, 16),
                    None => (number, 10),
                };
                // `readInt` takes digits only (no sign); an empty number isn't one.
                if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                    return None;
                }
                u32::from_str_radix(digits, radix)
                    .ok()
                    .and_then(char::from_u32)?
            } else {
                if body.chars().count() > 10 {
                    return None;
                }
                let at = ENTITIES
                    .binary_search_by(|(name, _)| (*name).cmp(body))
                    .ok()?;
                char::from_u32(ENTITIES[at].1)?
            };
            Some((c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => out.push('&'),
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}
