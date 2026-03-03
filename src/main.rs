use anyhow::{Context, Result, anyhow};
use pulldown_cmark::{Options, Parser, html};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

const PAGES_DIR: &str = "Pages";
const SPECIAL_DIR: &str = "Special";
const STATIC_DIR: &str = "static";
const OUTPUT_DIR: &str = "Meta\\site";

#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    title: Option<String>,
    tags: Option<Vec<String>>,
    draft: Option<bool>,
    slug: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct ManifestArticle {
    title: String,
    tags: Vec<String>,
    summary: Option<String>,
    source: String,
    url: String,
    category: String,
    subcategory: Vec<String>,
    slug: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug)]
struct GitDates {
    created_at: String,
    updated_at: String,
}

fn main() -> Result<()> {
    build_site()
}

fn build_site() -> Result<()> {
    let output_root = Path::new(OUTPUT_DIR);
    prepare_output_dir(output_root)?;
    copy_dir_contents_if_exists(Path::new(STATIC_DIR), output_root)?;
    render_special_pages(Path::new(SPECIAL_DIR), output_root)?;

    let articles = collect_articles(Path::new(PAGES_DIR), output_root)?;
    write_archive_page(output_root, &articles)?;
    write_manifest(output_root, &articles)?;

    println!("Generated {} article(s).", articles.len());
    Ok(())
}

fn prepare_output_dir(output_root: &Path) -> Result<()> {
    if output_root.exists() {
        fs::remove_dir_all(output_root).context("failed to clean output directory")?;
    }
    fs::create_dir_all(output_root).context("failed to create output directory")?;
    Ok(())
}

fn copy_dir_contents_if_exists(src_root: &Path, dest_root: &Path) -> Result<()> {
    if !src_root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(src_root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }

        let relative_path = entry
            .path()
            .strip_prefix(src_root)
            .context("failed to strip source prefix while copying files")?;
        let target_path = dest_root.join(relative_path);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target_path).with_context(|| {
            format!(
                "failed to copy '{}' to '{}'",
                entry.path().display(),
                target_path.display()
            )
        })?;
    }

    Ok(())
}

fn render_special_pages(special_root: &Path, output_root: &Path) -> Result<()> {
    if !special_root.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(special_root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }

        let relative_path = entry
            .path()
            .strip_prefix(special_root)
            .context("failed to strip Special/ prefix")?;
        let relative_path_str = relative_path.to_string_lossy().replace('\\', "/");

        if entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
        {
            let fragment = fs::read_to_string(entry.path()).with_context(|| {
                format!("failed to read special file '{}'", entry.path().display())
            })?;
            if is_full_html_document(&fragment) {
                return Err(anyhow!(
                    "Special/{} must be an HTML fragment, full HTML is not allowed",
                    relative_path_str
                ));
            }

            let title = if relative_path_str.eq_ignore_ascii_case("home.html") {
                "Home".to_string()
            } else {
                entry
                    .path()
                    .file_stem()
                    .map(|v| v.to_string_lossy().to_string())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "Page".to_string())
            };
            let wrapped = render_document_html(&title, &fragment);
            let target_path = if relative_path_str.eq_ignore_ascii_case("home.html") {
                output_root.join("index.html")
            } else {
                output_root.join(relative_path)
            };
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&target_path, wrapped).with_context(|| {
                format!(
                    "failed to write generated special page '{}'",
                    target_path.display()
                )
            })?;
            continue;
        }

        let target_path = output_root.join(relative_path);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target_path).with_context(|| {
            format!(
                "failed to copy special file '{}' to '{}'",
                entry.path().display(),
                target_path.display()
            )
        })?;
    }

    Ok(())
}

fn collect_articles(pages_root: &Path, output_root: &Path) -> Result<Vec<ManifestArticle>> {
    if !pages_root.exists() {
        return Ok(Vec::new());
    }

    let mut markdown_files = Vec::new();
    for entry in WalkDir::new(pages_root) {
        let entry = entry?;
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            markdown_files.push(entry.path().to_path_buf());
        }
    }
    markdown_files.sort();

    let mut articles = Vec::new();
    let mut used_urls = HashSet::new();

    for markdown_path in markdown_files {
        match process_markdown_file(&markdown_path, pages_root, output_root, &mut used_urls) {
            Ok(Some(article)) => articles.push(article),
            Ok(None) => {}
            Err(error) => {
                eprintln!("Skipped '{}': {error}", markdown_path.display());
            }
        }
    }

    Ok(articles)
}

fn process_markdown_file(
    file_path: &Path,
    pages_root: &Path,
    output_root: &Path,
    used_urls: &mut HashSet<String>,
) -> Result<Option<ManifestArticle>> {
    let source_raw = fs::read_to_string(file_path)
        .with_context(|| format!("failed to read markdown file '{}'", file_path.display()))?;

    let (frontmatter, body) = parse_frontmatter_and_body(&source_raw)?;
    let frontmatter = frontmatter.unwrap_or_default();
    if frontmatter.draft.unwrap_or(false) {
        return Ok(None);
    }

    let relative_path = file_path
        .strip_prefix(pages_root)
        .context("failed to strip Pages/ prefix")?;
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    let source_components: Vec<String> = parent
        .iter()
        .map(|part| part.to_string_lossy().to_string())
        .collect();

    if source_components.is_empty() {
        return Ok(None);
    }

    let file_stem = file_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string());
    let title = frontmatter.title.unwrap_or(file_stem);

    let mut section_slugs = Vec::with_capacity(source_components.len());
    for part in &source_components {
        section_slugs.push(slugify(part));
    }

    let requested_slug = frontmatter
        .slug
        .as_deref()
        .map(slugify)
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| slugify(&title));
    let article_slug = ensure_unique_article_slug(&requested_slug, &section_slugs, used_urls);
    let source = relative_path.to_string_lossy().replace('\\', "/");
    let git_dates = resolve_git_dates(&source)?;

    let output_dir = section_slugs
        .iter()
        .fold(output_root.to_path_buf(), |acc, part| acc.join(part))
        .join(&article_slug);
    fs::create_dir_all(&output_dir).with_context(|| {
        format!(
            "failed to create article output directory '{}'",
            output_dir.display()
        )
    })?;

    let html_body = markdown_to_html(&body);
    let breadcrumb_html = render_breadcrumbs(&source_components, &section_slugs, &title);
    let rendered = render_article_html(
        &title,
        frontmatter.summary.as_deref(),
        &frontmatter.tags.clone().unwrap_or_default(),
        &git_dates.created_at,
        if git_dates.created_at != git_dates.updated_at {
            Some(git_dates.updated_at.as_str())
        } else {
            None
        },
        &breadcrumb_html,
        &html_body,
    );
    fs::write(output_dir.join("index.html"), rendered).with_context(|| {
        format!(
            "failed to write generated page for '{}'",
            file_path.to_string_lossy()
        )
    })?;

    let url_path = format!(
        "/{}/",
        section_slugs
            .iter()
            .chain(std::iter::once(&article_slug))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("/")
    );

    Ok(Some(ManifestArticle {
        title,
        tags: frontmatter.tags.unwrap_or_default(),
        summary: frontmatter.summary,
        source,
        url: url_path,
        category: source_components[0].clone(),
        subcategory: source_components.into_iter().skip(1).collect(),
        slug: article_slug,
        created_at: git_dates.created_at,
        updated_at: git_dates.updated_at,
    }))
}

fn parse_frontmatter_and_body(raw: &str) -> Result<(Option<Frontmatter>, String)> {
    let mut lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() || lines[0].trim_end_matches('\r') != "---" {
        return Ok((None, raw.to_string()));
    }

    let end_marker = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (line.trim_end_matches('\r') == "---").then_some(index))
        .context("frontmatter delimiter is not closed")?;

    let yaml_text = lines[1..end_marker].join("\n");
    let body = if end_marker + 1 < lines.len() {
        lines.split_off(end_marker + 1).join("\n")
    } else {
        String::new()
    };

    let frontmatter: Frontmatter =
        serde_yaml::from_str(&yaml_text).context("failed to parse YAML frontmatter")?;
    Ok((Some(frontmatter), body))
}

fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_buf = String::new();
    html::push_html(&mut html_buf, parser);
    html_buf
}

fn render_article_html(
    title: &str,
    summary: Option<&str>,
    tags: &[String],
    created_at: &str,
    updated_at: Option<&str>,
    breadcrumb_html: &str,
    body_html: &str,
) -> String {
    let escaped_title = escape_html(title);
    let summary_block = summary
        .map(escape_html)
        .map(|value| format!("<p class=\"article-summary\">{value}</p>"))
        .unwrap_or_default();
    let tags_block = if tags.is_empty() {
        String::new()
    } else {
        let tag_items = tags
            .iter()
            .map(|tag| format!("<span class=\"tag-chip\">{}</span>", escape_html(tag)))
            .collect::<Vec<_>>()
            .join("");
        format!("<div class=\"article-tags\">{tag_items}</div>")
    };
    let escaped_created_at = escape_html(created_at);
    let display_created_at = escape_html(&to_display_date(created_at));
    let updated_block = updated_at
        .map(|date| {
            format!(
                " / <time datetime=\"{}\">更新: {}</time>",
                escape_html(date),
                escape_html(&to_display_date(date))
            )
        })
        .unwrap_or_default();
    let date_block = format!(
        "<p class=\"article-dates\"><time datetime=\"{escaped_created_at}\">作成: {display_created_at}</time>{updated_block}</p>"
    );
    let meta_block = format!("<div class=\"article-meta\">{date_block}{tags_block}</div>");

    let article_content = format!(
        "<main>\
<article class=\"article-card\">\
{breadcrumb_html}\
<h1 class=\"article-title\">{escaped_title}</h1>\
{meta_block}\
{summary_block}\
<div class=\"article-body\">{body_html}</div>\
</article>\
</main>"
    );

    render_document_html(title, &article_content)
}

fn render_breadcrumbs(components: &[String], section_slugs: &[String], title: &str) -> String {
    let mut items = vec![r#"<a href="/">Home</a>"#.to_string()];
    let mut path_segments = Vec::new();

    for (name, slug) in components.iter().zip(section_slugs.iter()) {
        path_segments.push(slug.as_str());
        let link = format!(
            r#"<a href="/{}/">{}</a>"#,
            path_segments.join("/"),
            escape_html(name)
        );
        items.push(link);
    }

    items.push(format!(
        "<span class=\"breadcrumb-current\">{}</span>",
        escape_html(title)
    ));
    format!(
        r#"<nav class="breadcrumbs" aria-label="breadcrumb">{}</nav>"#,
        items.join(" &gt; ")
    )
}

fn slugify(input: &str) -> String {
    let normalized = input.nfkc().collect::<String>().to_lowercase();

    let mut slug = String::with_capacity(normalized.len());
    let mut previous_was_dash = false;

    for ch in normalized.chars() {
        match ch {
            'a'..='z' | '0'..='9' => {
                slug.push(ch);
                previous_was_dash = false;
            }
            c if c.is_whitespace() || c == '-' => {
                if !previous_was_dash {
                    slug.push('-');
                    previous_was_dash = true;
                }
            }
            _ => {}
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        let hash = short_hash(input);
        format!("p-{hash}")
    } else {
        slug
    }
}

fn ensure_unique_article_slug(
    base_slug: &str,
    section_slugs: &[String],
    used_urls: &mut HashSet<String>,
) -> String {
    let mut sequence = 1usize;
    loop {
        let candidate = if sequence == 1 {
            base_slug.to_string()
        } else {
            format!("{base_slug}-{sequence}")
        };

        let full_url = section_slugs
            .iter()
            .chain(std::iter::once(&candidate))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("/");

        if used_urls.insert(full_url) {
            return candidate;
        }
        sequence += 1;
    }
}

fn short_hash(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut output = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn resolve_git_dates(relative_source_path: &str) -> Result<GitDates> {
    let updated_at = run_git_command(&["log", "-1", "--format=%cI", "--", relative_source_path])?;
    if updated_at.is_empty() {
        return Err(anyhow!(
            "no git history found for '{}'",
            relative_source_path
        ));
    }

    let created_history = run_git_command(&[
        "log",
        "--diff-filter=A",
        "--follow",
        "--format=%cI",
        "--",
        relative_source_path,
    ])?;
    let created_at = created_history
        .lines()
        .last()
        .map(str::to_string)
        .unwrap_or_else(|| updated_at.clone());

    Ok(GitDates {
        created_at,
        updated_at,
    })
}

fn run_git_command(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute git command: git {}", args.join(" ")))?;

    if !output.status.success() {
        return Err(anyhow!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn to_display_date(iso_datetime: &str) -> String {
    iso_datetime
        .split('T')
        .next()
        .unwrap_or(iso_datetime)
        .replace('-', "/")
}

fn render_document_html(title: &str, body_content: &str) -> String {
    let escaped_title = escape_html(title);
    format!(
        "<!doctype html>\
<html lang=\"ja\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{escaped_title}</title>\
<link rel=\"stylesheet\" href=\"/base.css\">\
</head>\
<body>\
<header class=\"site-header\">\
<a class=\"site-brand\" href=\"/\">Blog</a>\
<nav class=\"site-nav\"><a class=\"site-nav-link\" href=\"/\">Home</a><a class=\"site-nav-link\" href=\"/archive/\">Archive</a></nav>\
</header>\
<div class=\"content-container\">\
{body_content}\
</div>\
<footer class=\"site-footer\"><small>&copy; Blog</small></footer>\
</body>\
</html>"
    )
}

fn is_full_html_document(content: &str) -> bool {
    let normalized = content.to_ascii_lowercase();
    normalized.contains("<!doctype html") || normalized.contains("<html")
}

fn write_manifest(output_root: &Path, articles: &[ManifestArticle]) -> Result<()> {
    let manifest_path: PathBuf = output_root.join("manifest.json");
    let data = serde_json::to_string_pretty(articles).context("failed to serialize manifest")?;
    fs::write(&manifest_path, data).with_context(|| {
        format!(
            "failed to write manifest file '{}'",
            manifest_path.to_string_lossy()
        )
    })?;
    Ok(())
}

fn write_archive_page(output_root: &Path, articles: &[ManifestArticle]) -> Result<()> {
    let mut sorted = articles.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let archive_items = if sorted.is_empty() {
        "<p class=\"archive-empty\">No articles yet.</p>".to_string()
    } else {
        let items = sorted
            .iter()
            .map(|article| {
                let title = escape_html(&article.title);
                let url = escape_html(&article.url);
                let created_at_iso = escape_html(&article.created_at);
                let created_at_display = escape_html(&to_display_date(&article.created_at));
                let updated_block = if article.created_at != article.updated_at {
                    let updated_at_iso = escape_html(&article.updated_at);
                    let updated_at_display = escape_html(&to_display_date(&article.updated_at));
                    format!(
                        " / <time datetime=\"{updated_at_iso}\">更新: {updated_at_display}</time>"
                    )
                } else {
                    String::new()
                };
                let tags_block = if article.tags.is_empty() {
                    String::new()
                } else {
                    let tag_items = article
                        .tags
                        .iter()
                        .map(|tag| format!("<span class=\"tag-chip\">{}</span>", escape_html(tag)))
                        .collect::<Vec<_>>()
                        .join("");
                    format!("<div class=\"article-tags\">{tag_items}</div>")
                };
                format!(
                    "<li class=\"archive-item\"><div><a class=\"archive-link\" href=\"{url}\">{title}</a>{tags_block}</div><div class=\"archive-date\"><time datetime=\"{created_at_iso}\">作成: {created_at_display}</time>{updated_block}</div></li>"
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"archive-list\">{items}</ul>")
    };

    let archive_body = format!(
        "<main><section class=\"archive-section\"><h1 class=\"archive-title\">Archive</h1>{archive_items}</section></main>"
    );
    let html = render_document_html("Archive", &archive_body);

    let archive_dir = output_root.join("archive");
    fs::create_dir_all(&archive_dir).with_context(|| {
        format!(
            "failed to create archive output directory '{}'",
            archive_dir.display()
        )
    })?;
    fs::write(archive_dir.join("index.html"), html).context("failed to write archive page")?;
    Ok(())
}
