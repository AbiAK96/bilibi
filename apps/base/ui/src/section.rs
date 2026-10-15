//! Long pages split into sections that compile as separate templates.
//!
//! Askama inlines every macro call into the one render function of a template,
//! and the borrow checker's memory grows roughly with the square of a
//! function's size: the component specimen as one template made a single
//! `rustc`/`clippy-driver` process for this crate peak at 9 GB. A page that
//! calls many heavy macros (the form controls above all) is therefore cut into
//! section templates under `templates/<page>/`, each a verbatim slice of the
//! page that the page writes with `{{ self.<section>()? }}`.
//!
//! Never name a section file like a shared template (`tabs.html`,
//! `inline.html`, ...): askama resolves `{% import %}` beside the importing
//! file first, so the section would import itself.

/// Declares each section of `$page` rendered from its `$path`, and the `$page`
/// method of the same name that renders it. A section reads the page's fields
/// by their bare names through `Deref`, so its template text is exactly the
/// slice it replaces.
macro_rules! sections {
    ($page:ty { $($section:ident: $path:literal),+ $(,)? }) => {
        $(
            const _: () = {
                #[derive(askama::Template)]
                #[template(path = $path)]
                struct Section<'a>(&'a $page);

                impl std::ops::Deref for Section<'_> {
                    type Target = $page;

                    fn deref(&self) -> &$page {
                        self.0
                    }
                }

                impl $page {
                    fn $section(&self) -> askama::Result<$crate::Html> {
                        $crate::Html::render(&Section(self))
                    }
                }
            };
        )+
    };
}
pub(crate) use sections;
