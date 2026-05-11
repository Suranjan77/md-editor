# PDF Reader Integration: Research & Comparative Analysis

This document outlines the research, comparative analysis of different PDF rendering engines, and a final integration proposal for the Markdown Editor application. The goal is to integrate a high-performance PDF reader capable of advanced features like reference preview dialogs and Table of Contents (TOC) extraction.

---

## 1. Feature Requirements
Based on the application's needs, the selected PDF library must support:
- **Page Rendering**: High-quality rendering of PDF pages to image buffers.
- **Link/Annotation Extraction**: Finding the bounding boxes and destinations of internal links (e.g., figure and equation references).
- **Document Outline (Bookmarks)**: Extracting the Table of Contents tree with target destinations.
- **Text Extraction**: Fallback text searching for references that aren't properly hyperlinked.

---

## 2. Comparative Analysis of Approaches

We evaluated four primary approaches for integrating PDF capabilities, including both Rust-backend solutions and Frontend-only solutions.

### Approach 1: Rust Backend with `pdfium-render` (PDFium)
*PDFium is the open-source C++ PDF rendering engine developed by Google and used in Chrome.*

*   **Pros:**
    *   **Performance:** Exceptional speed and rendering accuracy.
    *   **Ergonomics:** The `pdfium-render` Rust crate provides safe, high-level, idiomatic abstractions over the C-API.
    *   **Feature Completeness:** Fully supports bookmarks, link extraction, bounding boxes, and text extraction.
    *   **License:** Permissive (Apache 2.0 / BSD), which is safe for both open-source and commercial applications without copyleft restrictions.
*   **Cons:**
    *   **Distribution:** Requires bundling pre-compiled PDFium binaries (`.dll`, `.so`, `.dylib`) with the Tauri application, slightly increasing the final binary size.

### Approach 2: Frontend-only with `pdf.js` (Mozilla)
*A popular pure-JavaScript PDF rendering library that draws to an HTML5 `<canvas>`.*

*   **Pros:**
    *   **Simplicity:** No Rust backend changes needed. Runs entirely in the Tauri Webview.
    *   **Cross-platform:** Works out of the box on all operating systems without bundling native binaries.
    *   **License:** Apache 2.0.
*   **Cons:**
    *   **Performance:** Can struggle with memory consumption and rendering speed on extremely large or complex scientific PDFs compared to native engines.
    *   **Main Thread Blocking:** Heavy parsing operations can block the webview's UI thread if not carefully offloaded to Web Workers.

### Approach 3: Rust Backend with `poppler-rs` (Poppler)
*Poppler is a widely used Linux PDF rendering library based on xpdf.*

*   **Pros:**
    *   Mature and heavily battle-tested on Linux systems.
*   **Cons:**
    *   **License:** GPL licensed. This strictly requires your application to be open-sourced under a GPL-compatible license.
    *   **Cross-platform Tooling:** Strongly tied to the GNOME/glib ecosystem, making it notoriously difficult to cross-compile or distribute reliably on Windows and macOS.

### Approach 4: Rust Backend with `mupdf` (MuPDF)
*A lightweight, extremely fast PDF engine written in C.*

*   **Pros:**
    *   Often faster and consumes less memory than PDFium or Poppler.
*   **Cons:**
    *   **License:** AGPL licensed. You must open-source your entire application under AGPL, or pay for an expensive commercial license from Artifex.
    *   **Rust Bindings:** Existing Rust bindings are less ergonomic and mature compared to `pdfium-render`.

---

## 3. Integration Proposal

**Recommendation:** We strongly recommend **Approach 1 (`pdfium-render`)**.

### Why `pdfium-render`?
1. **Licensing Safety**: Unlike Poppler (GPL) and MuPDF (AGPL), PDFium's permissive license ensures no legal restrictions are imposed on the Markdown Editor's distribution.
2. **Performance**: As a native Rust/C++ integration, it offloads heavy PDF parsing and rendering from the Tauri frontend, keeping the editor's UI smooth and responsive.
3. **API Fit**: `pdfium-render` provides exactly the APIs needed for the requested features:
   - `PdfPage::links()` for reference bounding boxes and destinations.
   - `PdfDocument::bookmarks()` for the Table of Contents tree.
   - The integrated `image` crate support makes it trivial to crop and return small preview images to the frontend.

### Implementation Architecture

1. **Backend (Rust / Tauri):**
   - Include `pdfium-render` in `src-tauri/Cargo.toml`.
   - Setup a build script or use an action to download pre-compiled PDFium binaries (e.g., from `bblanchon/pdfium-binaries`).
   - Create Tauri commands:
     - `get_pdf_metadata(path)`: Returns total pages and the JSON-serialized TOC.
     - `render_pdf_page(path, page_index, scale)`: Returns a base64 encoded image of the page.
     - `get_link_preview(path, page_index, x, y)`: Checks for a link at the given coordinates, resolves its destination, and returns a cropped base64 preview image of the target figure/equation.

2. **Frontend (JavaScript / HTML):**
   - Build a PDF Viewer component using an HTML `<canvas>` or `<img>` tag that dynamically loads pages from the Rust backend.
   - Attach a `contextmenu` (right-click) or `mouseenter` listener to the PDF container. Map screen coordinates to PDF coordinates and call `get_link_preview`. Display the returned image in an absolute-positioned floating dialog.
   - Add a global `keydown` listener for `t`. When pressed, render a sidebar containing the TOC JSON fetched during the initial load. Clicking an entry triggers a jump to the specified page index.

This architecture ensures a clean separation of concerns, high performance, and safe licensing.
