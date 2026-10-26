//! What a render hoists into the document's `<head>` (ADR-0010): React DOM's per-request queues
//! (`renderState` in react-dom-server 19.3.1), filled as elements are rendered and written in the
//! order `flushCompletedQueues` writes them.

use std::collections::HashSet;

use crate::html;

/// The `<head>` content a render collects: hoisted `<title>`, `<meta>` and `<link>` elements,
/// stylesheets by precedence, `async` scripts and image preloads, deduplicated as React
/// deduplicates them. It accumulates across renders until [`Head::clear`], like React's state
/// for one request.
#[derive(Debug, Default)]
pub struct Head {
    /// `<meta charSet>` (`charsetChunks`).
    charset: Vec<u8>,
    /// `<meta name="viewport">` (`viewportChunks`).
    viewport: Vec<u8>,
    /// The first ten image preloads, and every `fetchPriority="high"` one
    /// (`highImagePreloads`).
    high_image_preloads: Vec<u8>,
    high_image_count: usize,
    /// Stylesheets and style rules, one group per precedence in first-seen order (`styles`).
    styles: Vec<StyleGroup>,
    /// `async` scripts (`scripts`).
    scripts: Vec<u8>,
    /// Image preloads after the first ten (`bulkPreloads`), by key: a later image with the
    /// same key can still promote one to high priority (`promotablePreloads`).
    bulk_preloads: Vec<(String, Vec<u8>)>,
    /// `<title>`, other `<meta>`, and other `<link>` elements (`hoistableChunks`).
    hoistable: Vec<u8>,
    /// Stylesheet and style `href`s seen (`styleResources`).
    style_hrefs: HashSet<String>,
    /// Classic and module script `src`s seen (`scriptResources`, `moduleScriptResources`).
    script_srcs: HashSet<String>,
    module_script_srcs: HashSet<String>,
    /// Image preload keys seen (`imageResources`).
    image_keys: HashSet<String>,
}

/// One precedence's stylesheets and style rules.
#[derive(Debug)]
struct StyleGroup {
    precedence: String,
    /// `<link rel="stylesheet" … data-precedence>` elements.
    sheets: Vec<u8>,
    has_sheets: bool,
    /// `<style>` resources' `href`s and rules, written as one `<style>`.
    hrefs: Vec<String>,
    rules: Vec<u8>,
}

/// Where React puts a hoisted `<meta>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Meta {
    /// `charSet` is a string.
    Charset,
    /// `name="viewport"`.
    Viewport,
    /// Any other.
    Other,
}

impl Head {
    /// Whether nothing has been hoisted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.charset.is_empty()
            && self.viewport.is_empty()
            && self.high_image_preloads.is_empty()
            && self.styles.is_empty()
            && self.scripts.is_empty()
            && self.bulk_preloads.is_empty()
            && self.hoistable.is_empty()
    }

    /// Forgets everything, keeping capacity: the start of a new request.
    pub fn clear(&mut self) {
        self.charset.clear();
        self.viewport.clear();
        self.high_image_preloads.clear();
        self.high_image_count = 0;
        self.styles.clear();
        self.scripts.clear();
        self.bulk_preloads.clear();
        self.hoistable.clear();
        self.style_hrefs.clear();
        self.script_srcs.clear();
        self.module_script_srcs.clear();
        self.image_keys.clear();
    }

    /// Writes the queues in the order React DOM writes them into `<head>`
    /// (`flushCompletedQueues`): charset, viewport, high-priority image preloads, styles by
    /// precedence, scripts, bulk preloads, hoisted elements. Preconnects, font preloads, import
    /// maps and bootstrap scripts come from React DOM's resource APIs and Next.js, not from
    /// elements, so the runtime places them.
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.charset);
        out.extend_from_slice(&self.viewport);
        out.extend_from_slice(&self.high_image_preloads);
        for group in &self.styles {
            out.extend_from_slice(&group.sheets);
            // `flushStylesInPreamble`: the `<style>` is written unless the precedence only has
            // stylesheets.
            if !group.has_sheets || !group.hrefs.is_empty() {
                out.extend_from_slice(b"<style data-precedence=\"");
                html::escape(out, &group.precedence);
                if !group.hrefs.is_empty() {
                    out.extend_from_slice(b"\" data-href=\"");
                    for (i, href) in group.hrefs.iter().enumerate() {
                        if i > 0 {
                            out.push(b' ');
                        }
                        html::escape(out, href);
                    }
                }
                out.extend_from_slice(b"\">");
                out.extend_from_slice(&group.rules);
                out.extend_from_slice(b"</style>");
            }
        }
        out.extend_from_slice(&self.scripts);
        for (_, link) in &self.bulk_preloads {
            out.extend_from_slice(link);
        }
        out.extend_from_slice(&self.hoistable);
    }

    /// A hoisted `<title>` or non-stylesheet `<link>`.
    pub(crate) fn hoistable(&mut self, element: &[u8]) {
        self.hoistable.extend_from_slice(element);
    }

    /// A hoisted `<meta>`.
    pub(crate) fn meta(&mut self, element: &[u8], meta: Meta) {
        let queue = match meta {
            Meta::Charset => &mut self.charset,
            Meta::Viewport => &mut self.viewport,
            Meta::Other => &mut self.hoistable,
        };
        queue.extend_from_slice(element);
    }

    /// An `async` script, unless one with this `src` (and kind) was seen.
    pub(crate) fn script(&mut self, element: &[u8], src: &str, module: bool) {
        let seen = if module {
            &mut self.module_script_srcs
        } else {
            &mut self.script_srcs
        };
        if seen.insert(src.to_owned()) {
            self.scripts.extend_from_slice(element);
        }
    }

    /// A `<link rel="stylesheet">` with a precedence, unless its `href` was seen.
    pub(crate) fn stylesheet(&mut self, element: &[u8], precedence: &str, href: &str) {
        if self.style_hrefs.insert(href.to_owned()) {
            let group = self.group(precedence);
            group.sheets.extend_from_slice(element);
            group.has_sheets = true;
        }
    }

    /// A `<style>` with a precedence and `href`: its rules, unless its `href` was seen.
    pub(crate) fn style_rules(&mut self, rules: &[u8], precedence: &str, href: &str) {
        if self.style_hrefs.insert(href.to_owned()) {
            let group = self.group(precedence);
            group.hrefs.push(href.to_owned());
            group.rules.extend_from_slice(rules);
        }
    }

    /// Whether a stylesheet or style with this `href` was seen.
    pub(crate) fn has_style(&self, href: &str) -> bool {
        self.style_hrefs.contains(href)
    }

    /// An image preload: high priority for the first ten and for `fetchPriority="high"`, bulk
    /// otherwise. For a key seen before, nothing, except that a bulk preload is promoted to high
    /// priority by the same rule (the `img` case of `pushStartInstance`).
    pub(crate) fn image_preload(&mut self, link: &[u8], key: &str, high_priority: bool) {
        let room = high_priority || self.high_image_count < 10;
        if !self.image_keys.insert(key.to_owned()) {
            if room && let Some(at) = self.bulk_preloads.iter().position(|(k, _)| k == key) {
                let (_, promoted) = self.bulk_preloads.remove(at);
                self.high_image_preloads.extend_from_slice(&promoted);
                self.high_image_count += 1;
            }
            return;
        }
        if room {
            self.high_image_preloads.extend_from_slice(link);
            self.high_image_count += 1;
        } else {
            self.bulk_preloads.push((key.to_owned(), link.to_vec()));
        }
    }

    fn group(&mut self, precedence: &str) -> &mut StyleGroup {
        let at = self
            .styles
            .iter()
            .position(|g| g.precedence == precedence)
            .unwrap_or_else(|| {
                self.styles.push(StyleGroup {
                    precedence: precedence.to_owned(),
                    sheets: Vec::new(),
                    has_sheets: false,
                    hrefs: Vec::new(),
                    rules: Vec::new(),
                });
                self.styles.len() - 1
            });
        &mut self.styles[at]
    }
}
