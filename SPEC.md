# SPEC.md

Rust製の静的サイトジェネレータ

- 目的:  
  - `Pages/`に`Pages/<カテゴリ>/.../*.md`を放り込んでpushするだけでページの追加とサイトの更新をできるようにする  
  - 記事の作成日時と更新日時を表示できるようにする
  - トップページなどの特別なものは自動生成せず、HTMLをハードコードする
- 入力: ユーザーが唯一編集するMarkdown群（`Pages/`）
- 出力: 生成されたHTML等（`Meta/Site/`）
- カテゴリ: `Pages/`直下のフォルダ名（例: `研究ログ`）
- サブカテゴリ: カテゴリ配下の任意階層（例: `研究ログ/2026`）
- slug: URL用の安全文字列（表示は日本語、URLは安全文字列）

## ディレクトリ構造

```directory
Pages/                 # 入力（Markdown原本）
  研究ログ/
    AIの指示に100%従うチャレンジしたら全裸でベランダでコーディングする羽目になった話.md
Special/               # 特別ページ（HTML直書き）
  home.html            # トップページ
  404.html             # 404ページ
static/                # 静的資産
  base.css
  article.css
  icons/
    calendar.svg
Meta/                  # 出力（生成物）
  site/                # GitHub Pagesにデプロイする成果物
  cache/               # 差分ビルド用
```

## 入力ファイル仕様


