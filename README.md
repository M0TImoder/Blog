# Hi there! What are you doing here?

You’re a strange one for poking around here, aren't you?  
If you’re even weirder and actually want to read my ramblings, [click here]().  
Don't say I didn't warn you.  

PS: Everything here is a work in progress. Including me.  

## Generator notes (WIP)

- `Special/*.html` files are treated as HTML fragments (full HTML documents are rejected).
- `Special/home.html` is rendered to `/index.html` via the shared template.
- Markdown files without Git history are skipped with a warning.
- Recommended `Special/home.html` structure:

```html
<main>
  <section class="home-section">
    <h1 class="home-title">...</h1>
    <p class="home-lead">...</p>
  </section>
</main>
```

- Recommended `Special/404.html` structure:

```html
<main>
  <section>
    <h1>404</h1>
    <p>Not Found</p>
    <a href="/">Home</a>
  </section>
</main>
```

## Update flow (WIP)

1. Put `.md` files under `Pages/<カテゴリ>/...`.
2. Commit and push to `main` (or `master`).
3. GitHub Actions builds the site and deploys `Meta/site` to GitHub Pages.
