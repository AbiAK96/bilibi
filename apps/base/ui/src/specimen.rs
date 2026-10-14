//! The component specimen: every shared primitive and variant on one page,
//! served unauthenticated at `/assets/ui-components.html`. It is the screenshot
//! baseline for parity with Bilimbi's Design Library specimens, not the Design
//! Library itself. Secondary slots are built through the typed models, so the
//! page exercises both the macro and the Rust paths.
use crate::{
    ActionLink, BadgeKind, Button, ButtonVariant, Card, Chrome, Html, IconButton, IconContext,
    IconKind, PageVariant, StatItem,
};
use askama::Template;

#[derive(Template)]
#[template(path = "ui_components_specimen.html")]
pub struct ComponentsSpecimen {
    header_actions: Html,
    header_title_actions: Html,
    section_title_actions: Html,
    section_actions: Html,
    card_titled: Html,
    card_untitled: Html,
    card_flat: Html,
    stats_linked: Vec<StatItem>,
    stats_unlinked: Vec<StatItem>,
    stats_three: Vec<StatItem>,
}

impl ComponentsSpecimen {
    pub fn new() -> askama::Result<Self> {
        let header_actions = Html::render(&Button {
            variant: ButtonVariant::Primary,
            body: Html::text("Save changes"),
            ..Button::default()
        })?;
        let header_title_actions = Html::render(&IconButton {
            context: IconContext::Inline,
            ..IconButton::new("edit", "Edit company")
        })?;
        let section_title_actions = Html::render(&IconButton {
            context: IconContext::Inline,
            ..IconButton::new("edit", "Edit location")
        })?;
        let section_actions = Html::render(&ActionLink {
            id: "specimen-section-heading-manage",
            icon: "manage",
            href: "/system/design-library",
            title: "Manage this section's records",
            body: Html::text("Manage"),
            method: "",
        })?;
        let card_titled = Html::render(&Card {
            title: "Company profile",
            inner_class: "p-3",
            body: Html::raw(
                r#"<p class="text-sm text-ink">A title adds the bordered heading strip.</p>"#,
            ),
            ..Card::default()
        })?;
        let card_untitled = Html::render(&Card {
            inner_class: "p-3",
            body: Html::raw(
                r#"<p class="text-sm text-ink">Without one, the body stands alone.</p>"#,
            ),
            ..Card::default()
        })?;
        let card_flat = Html::render(&Card {
            inner_class: "p-0",
            body: Html::raw(
                r#"<p class="px-3 py-2 text-sm text-ink">p-0 drops the radius so a framed table picks up no corner.</p>"#,
            ),
            ..Card::default()
        })?;
        Ok(Self {
            header_actions,
            card_titled,
            card_untitled,
            card_flat,
            header_title_actions,
            section_title_actions,
            section_actions,
            stats_linked: vec![
                StatItem::number("People", 24),
                StatItem::number("Active", 18),
            ],
            stats_unlinked: vec![StatItem::text("Store", "Durable")],
            stats_three: vec![
                StatItem::number("Total", 62),
                StatItem::number("Active", 58),
                StatItem::text("Current", "MY"),
            ],
        })
    }
}
