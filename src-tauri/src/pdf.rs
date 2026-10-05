//! PDF 만들기: Typst 조판 엔진(프로그램 안에 포함, 설치 불필요)으로 PDF/A-2b 를 만든다.
//!
//! - 글꼴은 Windows 에 들어 있는 맑은 고딕을 쓰고, PDF 안에는 쓰인 글자만 넣는다(부분 포함).
//!   글꼴 파일 자체를 프로그램과 함께 배포하지 않는다.
//! - 서식(.typ)은 프로그램 안에 넣어 두고, 내용은 data.json 으로 넘긴다 (원문을 서식 코드에 섞지 않음).
//! - PDF/A 검사를 통과하지 못하는 글자(글꼴에 없는 특수 기호 등)가 있으면 일반 PDF 로 다시 만든다.

use crate::error::{AppError, AppResult};
use chrono::{Datelike, NaiveDate};
use std::path::PathBuf;
use std::sync::OnceLock;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;

/// 기본 글꼴 → 기호 → 이모지 순서로 찾는다
const FONT_FILES: &[&str] = &["malgun.ttf", "malgunbd.ttf", "malgunsl.ttf", "seguisym.ttf", "seguiemj.ttf"];

pub struct Fonts {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(w) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    // 사용자만 설치한 글꼴
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
    }
    dirs
}

fn load_fonts() -> Result<Fonts, String> {
    let mut fonts = Vec::new();
    let mut has_base = false;
    for name in FONT_FILES {
        let Some(path) = font_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file()) else { continue };
        let Ok(data) = std::fs::read(&path) else { continue };
        let before = fonts.len();
        fonts.extend(Font::iter(Bytes::new(data)));
        if *name == "malgun.ttf" && fonts.len() > before {
            has_base = true;
        }
    }
    if !has_base {
        return Err("PDF 에 쓸 한글 글꼴(맑은 고딕, malgun.ttf)을 찾지 못했습니다. Windows 글꼴 폴더를 확인해 주세요.".into());
    }
    let book = FontBook::from_fonts(&fonts);
    Ok(Fonts { book: LazyHash::new(book), fonts })
}

fn fonts() -> AppResult<&'static Fonts> {
    static FONTS: OnceLock<Result<Fonts, String>> = OnceLock::new();
    FONTS.get_or_init(load_fonts).as_ref().map_err(|e| AppError::msg(e.clone()))
}

fn file_id(path: &str) -> FileId {
    FileId::new(RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).expect("고정 경로")))
}

struct DocWorld {
    library: LazyHash<Library>,
    fonts: &'static Fonts,
    main_id: FileId,
    main: Source,
    data_id: FileId,
    data: Bytes,
    today: NaiveDate,
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &self.fonts.book
    }
    fn main(&self) -> FileId {
        self.main_id
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_id { Ok(self.main.clone()) } else { Err(FileError::NotSource) }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.data_id {
            Ok(self.data.clone())
        } else if id == self.main_id {
            Ok(Bytes::from_string(self.main.clone()))
        } else {
            // 서식은 프로그램 밖의 파일을 읽지 않는다
            Err(FileError::AccessDenied)
        }
    }
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        Datetime::from_ymd(self.today.year(), self.today.month() as u8, self.today.day() as u8)
    }
}

impl DocWorld {
    fn describe(&self, diags: &[SourceDiagnostic]) -> String {
        diags
            .iter()
            .take(5)
            .map(|d| {
                let line = self
                    .range(d.span)
                    .map(|r| self.main.text()[..r.start.min(self.main.text().len())].lines().count())
                    .map(|l| format!("{l}번째 줄: "))
                    .unwrap_or_default();
                let hints: Vec<String> = d.hints.iter().map(|h| h.v.to_string()).collect();
                if hints.is_empty() { format!("{line}{}", d.message) } else { format!("{line}{} ({})", d.message, hints.join(", ")) }
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

pub struct Rendered {
    pub bytes: Vec<u8>,
    pub pages: usize,
    /// PDF/A-2b 로 만들었는지 (아니면 일반 PDF)
    pub pdfa: bool,
    /// PDF/A 로 만들지 못한 이유
    pub note: Option<String>,
}

fn compile(template: &str, data_json: String, today: NaiveDate) -> AppResult<(DocWorld, PagedDocument)> {
    let fonts = fonts()?;
    let main_id = file_id("/main.typ");
    let world = DocWorld {
        library: LazyHash::new(Library::default()),
        fonts,
        main_id,
        main: Source::new(main_id, template.to_string()),
        data_id: file_id("/data.json"),
        data: Bytes::from_string(data_json),
        today,
    };
    let doc: PagedDocument = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|e| AppError::msg(format!("PDF 서식을 조판하지 못했습니다: {}", world.describe(&e))))?;
    Ok((world, doc))
}

/// 개발 중 눈으로 확인하기 위한 쪽 그림 (배포 프로그램에는 들어가지 않음)
#[cfg(test)]
pub fn render_pngs(template: &str, data_json: String, today: NaiveDate) -> AppResult<Vec<Vec<u8>>> {
    let (_, doc) = compile(template, data_json, today)?;
    let opts = typst_render::RenderOptions { pixel_per_pt: typst::utils::Scalar::new(1.4), render_bleed: false };
    Ok(doc.pages().iter().map(|p| typst_render::render(p, &opts).encode_png().unwrap_or_default()).collect())
}

/// 서식과 JSON 자료로 PDF 를 만든다. PDF/A 가 안 되면 일반 PDF 로 다시 만든다.
pub fn render(template: &str, data_json: String, today: NaiveDate) -> AppResult<Rendered> {
    let (world, doc) = compile(template, data_json, today)?;
    let pages = doc.pages().len();

    let mut opts = typst_pdf::PdfOptions::default();
    opts.creator = typst::foundations::Smart::Custom(Some(format!("로컬 AI 교무수첩 {}", env!("CARGO_PKG_VERSION"))));
    opts.timestamp = Datetime::from_ymd(today.year(), today.month() as u8, today.day() as u8).map(typst_pdf::Timestamp::new_utc);
    opts.standards = typst_pdf::PdfStandards::new(&[typst_pdf::PdfStandard::A_2b])
        .map_err(|e| AppError::msg(format!("PDF/A 설정 오류: {}", e.message())))?;
    match typst_pdf::pdf(&doc, &opts) {
        Ok(bytes) => Ok(Rendered { bytes, pages, pdfa: true, note: None }),
        Err(e) => {
            let why = world.describe(&e);
            opts.standards = typst_pdf::PdfStandards::default();
            let bytes = typst_pdf::pdf(&doc, &opts).map_err(|e| AppError::msg(format!("PDF 를 만들지 못했습니다: {}", world.describe(&e))))?;
            Ok(Rendered { bytes, pages, pdfa: false, note: Some(why) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean_pdfa_with_json_data() {
        let tpl = r#"
#let data = json("data.json")
#set document(title: data.title)
#set text(font: "Malgun Gothic", lang: "ko", size: 10pt)
= #data.title
#for s in data.items [ - #s \ ]
#table(columns: 2, [학급], [이름], ..data.rows.flatten())
"#;
        let data = serde_json::json!({
            "title": "2026학년도 교무수첩 기록",
            "items": ["홍길동 발표를 잘함, 칭찬함", "#include \"x\" 같은 글도 그대로 글자로"],
            "rows": [["2-1", "홍길동"], ["2-2", "성춘향"]],
        });
        let r = render(tpl, data.to_string(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()).unwrap();
        assert!(r.bytes.starts_with(b"%PDF-"));
        assert!(r.pdfa, "{:?}", r.note);
        assert_eq!(r.pages, 1);
        // PDF/A 식별 정보(XMP pdfaid)가 들어 있다
        let s = String::from_utf8_lossy(&r.bytes);
        assert!(s.contains("pdfaid"), "PDF/A 표시 없음");
    }

    #[test]
    fn template_errors_are_reported_in_korean() {
        let e = render("#let x = (", "{}".into(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()).err().unwrap();
        assert!(e.to_string().contains("PDF 서식을 조판하지 못했습니다"), "{e}");
    }

    #[test]
    fn template_cannot_read_other_files() {
        let e = render("#read(\"C:/Windows/win.ini\")", "{}".into(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()).err();
        assert!(e.is_some());
    }
}
