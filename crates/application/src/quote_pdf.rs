//! Quote documents. `quote_pdf` is a pure function, so tests need no window.
//!
//! Page coordinates are floats in points; money is formatted from integer
//! cents before it gets here.

use std::{
    fs,
    path::{Path, PathBuf},
};

use cominorsa_domain::{CompanyProfile, Quote, QuoteStatus};
use krilla::{
    Document,
    color::rgb,
    geom::{PathBuilder, Point, Rect},
    metadata::Metadata,
    page::PageSettings,
    paint::Fill,
    surface::Surface,
    text::{Font, TextDirection},
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};

use crate::presentation::{QUOTE_TAX_NOTE, company_lines, format_date, format_soles, igv_label};

const REGULAR_FONT: &[u8] = include_bytes!("../../desktop/assets/fonts/NotoSans-Regular.ttf");
const BOLD_FONT: &[u8] = include_bytes!("../../desktop/assets/fonts/NotoSans-Bold.ttf");

const PAGE_WIDTH: f32 = 595.28;
const PAGE_HEIGHT: f32 = 841.89;
const MARGIN: f32 = 56.0;
const RIGHT: f32 = PAGE_WIDTH - MARGIN;
const CONTENT_WIDTH: f32 = RIGHT - MARGIN;
const TOP: f32 = 64.0;
const FOOTER_Y: f32 = PAGE_HEIGHT - 36.0;
const BOTTOM: f32 = FOOTER_Y - 24.0;

const BODY_SIZE: f32 = 9.5;
const SMALL_SIZE: f32 = 8.5;
const HEADING_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 20.0;
const LINE_HEIGHT: f32 = 13.5;
const BLOCK_GAP: f32 = 16.0;
const RULE_THICKNESS: f32 = 0.6;

const COMPANY_WIDTH: f32 = 300.0;
const DESCRIPTION_WIDTH: f32 = 214.0;
const QUANTITY_RIGHT: f32 = 322.0;
const UNIT_PRICE_RIGHT: f32 = 432.0;
const TOTAL_LABEL_RIGHT: f32 = 432.0;

const INK: (u8, u8, u8) = (28, 30, 33);
const MUTED: (u8, u8, u8) = (88, 92, 98);
const RULE: (u8, u8, u8) = (190, 194, 199);

#[derive(Clone, Copy)]
enum Weight {
    Regular,
    Bold,
}

struct Typeface {
    font: Font,
    face: FontRef<'static>,
}

impl Typeface {
    fn load(data: &'static [u8]) -> Result<Self, String> {
        let unreadable = || "No se pudo leer la fuente del PDF".to_owned();

        Ok(Self {
            font: Font::new(data.into(), 0).ok_or_else(unreadable)?,
            face: FontRef::new(data).map_err(|_| unreadable())?,
        })
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        let charmap = self.face.charmap();
        let metrics = self
            .face
            .glyph_metrics(Size::new(size), LocationRef::default());

        text.chars()
            .filter_map(|character| charmap.map(character))
            .filter_map(|glyph| metrics.advance_width(glyph))
            .sum()
    }
}

struct Typefaces {
    regular: Typeface,
    bold: Typeface,
}

impl Typefaces {
    fn of(&self, weight: Weight) -> &Typeface {
        match weight {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        }
    }
}

enum Mark {
    Text {
        x: f32,
        y: f32,
        size: f32,
        weight: Weight,
        color: (u8, u8, u8),
        text: String,
    },
    Rule {
        y: f32,
    },
}

/// Places text top to bottom, starting a new page when it runs out of room.
struct Layout<'a> {
    typefaces: &'a Typefaces,
    pages: Vec<Vec<Mark>>,
    y: f32,
    in_table: bool,
}

impl<'a> Layout<'a> {
    fn new(typefaces: &'a Typefaces) -> Self {
        Self {
            typefaces,
            pages: vec![Vec::new()],
            y: TOP,
            in_table: false,
        }
    }

    fn mark(&mut self, mark: Mark) {
        if let Some(page) = self.pages.last_mut() {
            page.push(mark);
        }
    }

    fn text(&mut self, x: f32, size: f32, weight: Weight, color: (u8, u8, u8), text: &str) {
        self.mark(Mark::Text {
            x,
            y: self.y,
            size,
            weight,
            color,
            text: text.to_owned(),
        });
    }

    fn right_text(&mut self, right: f32, size: f32, weight: Weight, text: &str) {
        let x = right - self.typefaces.of(weight).width(text, size);
        self.text(x, size, weight, INK, text);
    }

    fn rule(&mut self) {
        self.mark(Mark::Rule { y: self.y });
    }

    fn ensure(&mut self, height: f32) {
        if self.y + height > BOTTOM {
            self.pages.push(Vec::new());
            self.y = TOP;
            if self.in_table {
                self.table_header();
            }
        }
    }

    /// Breaks text at spaces, and inside a word wider than the line.
    fn wrap(&self, text: &str, width: f32, size: f32, weight: Weight) -> Vec<String> {
        let typeface = self.typefaces.of(weight);
        let fits = |candidate: &str| typeface.width(candidate, size) <= width;
        let mut lines = Vec::new();

        for paragraph in text.lines() {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let joined = format!("{line} {word}");
                if !line.is_empty() && fits(&joined) {
                    line = joined;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                for character in word.chars() {
                    let mut longer = line.clone();
                    longer.push(character);
                    if !line.is_empty() && !fits(&longer) {
                        lines.push(std::mem::take(&mut line));
                        longer = character.to_string();
                    }
                    line = longer;
                }
            }
            lines.push(line);
        }
        lines
    }

    fn wrapped(&mut self, x: f32, width: f32, size: f32, weight: Weight, text: &str) {
        for line in self.wrap(text, width, size, weight) {
            self.ensure(LINE_HEIGHT);
            self.text(x, size, weight, INK, &line);
            self.y += LINE_HEIGHT;
        }
    }

    fn header(&mut self, quote: &Quote, company: &CompanyProfile) {
        self.right_text(RIGHT, TITLE_SIZE, Weight::Bold, "Cotización");
        let start = self.y;
        self.y += LINE_HEIGHT + 4.0;
        self.right_text(
            RIGHT,
            BODY_SIZE,
            Weight::Regular,
            &quote.number().to_string(),
        );
        let below_title = self.y + LINE_HEIGHT;

        self.y = start;
        if company.has_name() {
            self.wrapped(
                MARGIN,
                COMPANY_WIDTH,
                HEADING_SIZE,
                Weight::Bold,
                company.name.trim(),
            );
        }
        for line in company_lines(company) {
            self.wrapped(MARGIN, COMPANY_WIDTH, SMALL_SIZE, Weight::Regular, &line);
        }
        self.y = self.y.max(below_title) + BLOCK_GAP;
    }

    fn details(&mut self, quote: &Quote) {
        self.rule();
        self.y += BLOCK_GAP;
        self.wrapped(
            MARGIN,
            CONTENT_WIDTH,
            BODY_SIZE,
            Weight::Bold,
            quote.client_name(),
        );

        let mut facts = Vec::new();
        if let Some(ruc) = quote.client_ruc() {
            facts.push(format!("RUC {}", ruc.as_str()));
        }
        facts.push(format!("Servicio: {}", quote.service()));
        facts.push(format!(
            "Fecha de emisión: {}",
            format_date(quote.issued_on())
        ));
        facts.push(format!(
            "Válida hasta: {}",
            format_date(quote.valid_until())
        ));
        if quote.status() == QuoteStatus::Draft {
            facts.push("Borrador: documento aún no emitido".to_owned());
        }
        for fact in facts {
            self.wrapped(MARGIN, CONTENT_WIDTH, BODY_SIZE, Weight::Regular, &fact);
        }
        self.y += BLOCK_GAP;
    }

    fn table_header(&mut self) {
        self.text(MARGIN, SMALL_SIZE, Weight::Bold, MUTED, "Descripción");
        self.right_text(QUANTITY_RIGHT, SMALL_SIZE, Weight::Bold, "Cant.");
        self.right_text(UNIT_PRICE_RIGHT, SMALL_SIZE, Weight::Bold, "P. unitario");
        self.right_text(RIGHT, SMALL_SIZE, Weight::Bold, "Importe");
        self.y += 6.0;
        self.rule();
        self.y += LINE_HEIGHT;
    }

    fn lines(&mut self, quote: &Quote) {
        self.ensure(LINE_HEIGHT * 3.0);
        self.table_header();
        self.in_table = true;

        for line in quote.lines() {
            let wrapped = self.wrap(
                line.description(),
                DESCRIPTION_WIDTH,
                BODY_SIZE,
                Weight::Regular,
            );
            let height = LINE_HEIGHT * wrapped.len() as f32 + 4.0;
            self.ensure(height.min(LINE_HEIGHT * 2.0));
            self.right_text(
                QUANTITY_RIGHT,
                BODY_SIZE,
                Weight::Regular,
                &line.quantity().to_string(),
            );
            self.right_text(
                UNIT_PRICE_RIGHT,
                BODY_SIZE,
                Weight::Regular,
                &format_soles(line.unit_price()),
            );
            self.right_text(
                RIGHT,
                BODY_SIZE,
                Weight::Regular,
                &format_soles(line.total()),
            );
            for text in wrapped {
                self.ensure(LINE_HEIGHT);
                self.text(MARGIN, BODY_SIZE, Weight::Regular, INK, &text);
                self.y += LINE_HEIGHT;
            }
            self.y += 4.0;
        }

        self.in_table = false;
        self.rule();
        self.y += LINE_HEIGHT;
    }

    fn totals(&mut self, quote: &Quote) {
        let totals = quote.totals();
        self.ensure(LINE_HEIGHT * 5.0);

        for (label, amount, weight) in [
            ("Subtotal".to_owned(), totals.subtotal, Weight::Regular),
            (igv_label(quote.igv_rate()), totals.igv, Weight::Regular),
            ("Total".to_owned(), totals.total, Weight::Bold),
        ] {
            self.right_text(TOTAL_LABEL_RIGHT, BODY_SIZE, weight, &label);
            self.right_text(RIGHT, BODY_SIZE, weight, &format_soles(amount));
            self.y += LINE_HEIGHT;
        }
        self.text(
            MARGIN,
            SMALL_SIZE,
            Weight::Regular,
            MUTED,
            "Importes en soles. Los precios unitarios no incluyen IGV.",
        );
        self.y += LINE_HEIGHT + BLOCK_GAP;
    }

    fn notes(&mut self, quote: &Quote) {
        if quote.notes().is_empty() {
            return;
        }

        self.ensure(LINE_HEIGHT * 2.0);
        self.text(MARGIN, SMALL_SIZE, Weight::Bold, MUTED, "Notas");
        self.y += LINE_HEIGHT;
        self.wrapped(
            MARGIN,
            CONTENT_WIDTH,
            BODY_SIZE,
            Weight::Regular,
            quote.notes(),
        );
    }

    /// Every page states what the document is, even when printed apart.
    fn footers(&mut self) {
        let count = self.pages.len();
        let typeface = self.typefaces.of(Weight::Regular);

        for (index, page) in self.pages.iter_mut().enumerate() {
            let label = format!("Página {} de {count}", index + 1);
            let right = RIGHT - typeface.width(&label, SMALL_SIZE);
            page.push(Mark::Text {
                x: MARGIN,
                y: FOOTER_Y,
                size: SMALL_SIZE,
                weight: Weight::Regular,
                color: MUTED,
                text: QUOTE_TAX_NOTE.to_owned(),
            });
            page.push(Mark::Text {
                x: right,
                y: FOOTER_Y,
                size: SMALL_SIZE,
                weight: Weight::Regular,
                color: MUTED,
                text: label,
            });
        }
    }
}

fn fill((red, green, blue): (u8, u8, u8)) -> Option<Fill> {
    Some(Fill {
        paint: rgb::Color::new(red, green, blue).into(),
        ..Fill::default()
    })
}

fn draw(surface: &mut Surface<'_>, typefaces: &Typefaces, mark: &Mark) {
    match mark {
        Mark::Text {
            x,
            y,
            size,
            weight,
            color,
            text,
        } => {
            surface.set_fill(fill(*color));
            surface.draw_text(
                Point::from_xy(*x, *y),
                typefaces.of(*weight).font.clone(),
                *size,
                text,
                false,
                TextDirection::Auto,
            );
        }
        Mark::Rule { y } => {
            let mut builder = PathBuilder::new();
            if let Some(rect) = Rect::from_xywh(MARGIN, *y, CONTENT_WIDTH, RULE_THICKNESS) {
                builder.push_rect(rect);
            }
            if let Some(path) = builder.finish() {
                surface.set_fill(fill(RULE));
                surface.draw_path(&path);
            }
        }
    }
}

/// Renders a quote. Blank company fields are left out. No date of creation is
/// embedded, so the same quote always produces the same bytes.
pub fn quote_pdf(quote: &Quote, company: &CompanyProfile) -> Result<Vec<u8>, String> {
    let failed = "No se pudo generar el PDF".to_owned();
    let typefaces = Typefaces {
        regular: Typeface::load(REGULAR_FONT)?,
        bold: Typeface::load(BOLD_FONT)?,
    };

    let mut layout = Layout::new(&typefaces);
    layout.header(quote, company);
    layout.details(quote);
    layout.lines(quote);
    layout.totals(quote);
    layout.notes(quote);
    layout.footers();

    let mut document = Document::new();
    document.set_metadata(
        Metadata::new()
            .title(format!("Cotización {}", quote.number()))
            .language("es-PE".to_owned()),
    );
    for marks in &layout.pages {
        let settings = PageSettings::from_wh(PAGE_WIDTH, PAGE_HEIGHT).ok_or(failed.clone())?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        for mark in marks {
            draw(&mut surface, &typefaces, mark);
        }
        surface.finish();
        page.finish();
    }

    document.finish().map_err(|_| failed)
}

/// Writes the document where the person chose. Some file dialogs return the
/// name without its extension.
/// The file a save to `path` writes: without an extension, it gets `.pdf`.
pub(crate) fn pdf_path(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        path.with_extension("pdf")
    }
}

pub fn write_pdf(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = pdf_path(path);

    fs::write(&path, bytes).map_err(|error| format!("No se pudo guardar el PDF: {error}"))?;
    Ok(path)
}
