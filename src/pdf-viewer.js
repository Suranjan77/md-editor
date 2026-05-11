/**
 * PDF Viewer Component
 *
 * A high-performance PDF viewer that renders pages via the Rust/PDFium backend.
 * Features:
 * - Virtual scrolling with IntersectionObserver (only renders visible pages)
 * - Zoom controls
 * - Table of Contents panel (Ctrl+T)
 * - Link detection and preview on right-click
 * - In-PDF text search (Ctrl+F when focused)
 */

import {
  openPdf,
  closePdf,
  getPageLinks,
  getLinkPreview,
  searchPdf,
} from "./ipc.js";

// ── State ───────────────────────────────────────────────────────────

let container = null;
let pagesContainer = null;
let metadata = null;
let currentScale = 1.5;
let currentPath = null;
let pageElements = [];
let observer = null;
let loadingPages = new Set();
let tocVisible = false;
let searchVisible = false;
let linkOverlays = new Map(); // pageIndex -> [{bbox, destPage, uri, el}]
let previewTooltip = null;

const SCALE_STEP = 0.25;
const MIN_SCALE = 0.5;
const MAX_SCALE = 4.0;
const OBSERVER_MARGIN = "200px"; // Pre-load pages 200px before they enter viewport

// ── Public API ──────────────────────────────────────────────────────

/**
 * Initialize the PDF viewer in the given container element.
 */
export function initPdfViewer(hostElement) {
  container = hostElement;
  container.innerHTML = "";
  container.classList.add("pdf-viewer");

  // Build the viewer skeleton
  container.innerHTML = `
    <div class="pdf-body">
      <div id="pdf-toc-panel" class="pdf-toc-panel hidden">
        <div class="pdf-toc-header">
          <span>Contents</span>
          <button id="pdf-toc-close" class="pdf-tool-btn pdf-tool-btn-sm">
            <span class="material-symbols-outlined">close</span>
          </button>
        </div>
        <div id="pdf-toc-tree" class="pdf-toc-tree"></div>
      </div>

      <div class="pdf-scroll-area" id="pdf-scroll-area">
        <div id="pdf-search-bar" class="pdf-search-bar hidden">
          <input type="text" id="pdf-search-input" placeholder="Search in PDF..." />
          <span id="pdf-search-count" class="pdf-search-count"></span>
          <button id="pdf-search-prev" class="pdf-tool-btn pdf-tool-btn-sm">
            <span class="material-symbols-outlined">expand_less</span>
          </button>
          <button id="pdf-search-next" class="pdf-tool-btn pdf-tool-btn-sm">
            <span class="material-symbols-outlined">expand_more</span>
          </button>
          <button id="pdf-search-close" class="pdf-tool-btn pdf-tool-btn-sm">
            <span class="material-symbols-outlined">close</span>
          </button>
        </div>
        <div id="pdf-pages" class="pdf-pages"></div>
      </div>
    </div>

    <div class="pdf-toolbar">
      <div class="pdf-toolbar-left">
        <button id="pdf-toc-btn" class="pdf-tool-btn" title="Table of Contents (Ctrl+T)">
          <span class="material-symbols-outlined">toc</span>
        </button>
        <button id="pdf-search-btn" class="pdf-tool-btn" title="Search in PDF (Ctrl+F)">
          <span class="material-symbols-outlined">search</span>
        </button>
      </div>
      <div class="pdf-toolbar-center">
        <button id="pdf-zoom-out" class="pdf-tool-btn" title="Zoom Out">
          <span class="material-symbols-outlined">remove</span>
        </button>
        <span id="pdf-zoom-level" class="pdf-zoom-label">150%</span>
        <button id="pdf-zoom-in" class="pdf-tool-btn" title="Zoom In">
          <span class="material-symbols-outlined">add</span>
        </button>
        <button id="pdf-zoom-fit" class="pdf-tool-btn" title="Fit Width">
          <span class="material-symbols-outlined">fit_width</span>
        </button>
      </div>
      <div class="pdf-toolbar-right">
        <span id="pdf-page-info" class="pdf-page-info"></span>
      </div>
    </div>

    <div id="pdf-link-backdrop" class="pdf-preview-backdrop hidden"></div>
    <div id="pdf-link-preview" class="pdf-link-preview hidden"></div>
  `;

  pagesContainer = container.querySelector("#pdf-pages");
  previewTooltip = container.querySelector("#pdf-link-preview");
  const backdrop = container.querySelector("#pdf-link-backdrop");

  // Click backdrop to dismiss preview
  backdrop.addEventListener("click", hidePreview);

  // Wire up controls
  container.querySelector("#pdf-zoom-in").addEventListener("click", zoomIn);
  container.querySelector("#pdf-zoom-out").addEventListener("click", zoomOut);
  container.querySelector("#pdf-zoom-fit").addEventListener("click", zoomFitWidth);
  container.querySelector("#pdf-toc-btn").addEventListener("click", toggleToc);
  container.querySelector("#pdf-toc-close").addEventListener("click", toggleToc);
  container.querySelector("#pdf-search-btn").addEventListener("click", toggleSearch);
  container.querySelector("#pdf-search-close").addEventListener("click", toggleSearch);
  container.querySelector("#pdf-search-input").addEventListener("input", debounce(handlePdfSearch, 400));
  container.querySelector("#pdf-search-prev").addEventListener("click", () => navigateSearchResult(-1));
  container.querySelector("#pdf-search-next").addEventListener("click", () => navigateSearchResult(1));

  // Keyboard shortcuts when viewer is focused
  container.addEventListener("keydown", handleViewerKeydown);

  // Track scroll for page indicator
  const scrollArea = container.querySelector("#pdf-scroll-area");
  scrollArea.addEventListener("scroll", debounce(updatePageIndicator, 100));
}

/**
 * Open and display a PDF file.
 */
export async function openPdfFile(relativePath) {
  if (!container) return;

  currentPath = relativePath;
  pagesContainer.innerHTML = "";
  pageElements = [];
  loadingPages.clear();
  linkOverlays.clear();

  // Disconnect old observer
  if (observer) observer.disconnect();

  try {
    metadata = await openPdf(relativePath);
  } catch (err) {
    pagesContainer.innerHTML = `
      <div class="pdf-error">
        <span class="material-symbols-outlined">error</span>
        <p>Failed to open PDF</p>
        <p class="pdf-error-detail">${err}</p>
      </div>
    `;
    return;
  }

  updateZoomLabel();
  updatePageInfo(0);

  // Build TOC
  renderToc(metadata.toc);

  // Create placeholder elements for each page
  for (let i = 0; i < metadata.total_pages; i++) {
    const pageWrapper = document.createElement("div");
    pageWrapper.className = "pdf-page-wrapper";
    pageWrapper.setAttribute("data-page-index", i);

    // Set initial size based on metadata and scale
    const scaledWidth = metadata.page_width * currentScale;
    const scaledHeight = metadata.page_height * currentScale;
    pageWrapper.style.width = `${scaledWidth}px`;
    pageWrapper.style.height = `${scaledHeight}px`;

    const pageNum = document.createElement("div");
    pageNum.className = "pdf-page-number";
    pageNum.textContent = i + 1;

    const canvas = document.createElement("div");
    canvas.className = "pdf-page-canvas";

    // Spinner placeholder
    canvas.innerHTML = `
      <div class="pdf-page-loading">
        <div class="pdf-spinner"></div>
      </div>
    `;

    pageWrapper.appendChild(canvas);
    pageWrapper.appendChild(pageNum);
    pagesContainer.appendChild(pageWrapper);
    pageElements.push(pageWrapper);
  }

  // Set up IntersectionObserver for virtual rendering
  setupObserver();
}

/**
 * Close the current PDF and clean up.
 */
export async function closePdfViewer() {
  if (observer) observer.disconnect();
  if (metadata) {
    try {
      await closePdf();
    } catch (e) {
      console.warn("Failed to close PDF:", e);
    }
  }
  metadata = null;
  currentPath = null;
  pageElements = [];
  loadingPages.clear();
  linkOverlays.clear();
  if (pagesContainer) pagesContainer.innerHTML = "";
  hideToc();
  hideSearch();
}

/**
 * Check if the PDF viewer is currently active (has an open document).
 */
export function isPdfViewerActive() {
  return metadata !== null;
}

// ── IntersectionObserver (Virtual Scroll) ────────────────────────────

function setupObserver() {
  observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          const pageIndex = parseInt(entry.target.getAttribute("data-page-index"));
          renderPage(pageIndex);
        }
      }
    },
    {
      root: container.querySelector("#pdf-scroll-area"),
      rootMargin: OBSERVER_MARGIN,
    }
  );

  pageElements.forEach((el) => observer.observe(el));
}

async function renderPage(pageIndex) {
  if (loadingPages.has(pageIndex)) return;

  const wrapper = pageElements[pageIndex];
  if (!wrapper) return;

  // Check if already rendered at current scale
  const renderedScale = wrapper.getAttribute("data-rendered-scale");
  if (renderedScale === String(currentScale)) return;

  loadingPages.add(pageIndex);

  try {
    const canvas = wrapper.querySelector(".pdf-page-canvas");
    const img = document.createElement("img");
    
    // Convert scale to integer percentage
    const scaleInt = Math.round(currentScale * 100);
    
    await new Promise((resolve, reject) => {
      img.onload = resolve;
      img.onerror = () => reject(new Error("Image failed to load from custom URI"));
      img.src = `md-pdf://localhost/${pageIndex}/${scaleInt}`;
    });

    img.className = "pdf-page-img";
    img.alt = `Page ${pageIndex + 1}`;
    img.draggable = false;

    // Replace loading spinner with rendered image
    canvas.innerHTML = "";
    canvas.appendChild(img);

    wrapper.setAttribute("data-rendered-scale", String(currentScale));

    // Load link overlays for this page
    loadPageLinks(pageIndex, canvas);
  } catch (err) {
    console.error(`Failed to render page ${pageIndex}:`, err);
    const canvas = wrapper.querySelector(".pdf-page-canvas");
    canvas.innerHTML = `<div class="pdf-page-error">Failed to render</div>`;
  } finally {
    loadingPages.delete(pageIndex);
  }
}

// ── Link Overlays ───────────────────────────────────────────────────

async function loadPageLinks(pageIndex, canvasEl) {
  try {
    const links = await getPageLinks(pageIndex);
    if (!links || links.length === 0) return;

    const overlayContainer = document.createElement("div");
    overlayContainer.className = "pdf-link-overlay-container";

    const storedLinks = [];

    for (const link of links) {
      const el = document.createElement("div");
      el.className = "pdf-link-region";
      el.style.left = `${link.bbox.x * currentScale}px`;
      el.style.top = `${link.bbox.y * currentScale}px`;
      el.style.width = `${link.bbox.width * currentScale}px`;
      el.style.height = `${link.bbox.height * currentScale}px`;

      if (link.uri) {
        el.title = link.uri;
        el.style.cursor = "pointer";
        el.addEventListener("click", () => {
          window.open(link.uri, "_blank");
        });
      } else if (link.dest_page !== null && link.dest_page !== undefined) {
        el.title = `Go to page ${link.dest_page + 1}`;
        el.style.cursor = "pointer";
        el.addEventListener("click", () => {
          scrollToPage(link.dest_page);
        });

        // Right-click for preview
        el.addEventListener("contextmenu", async (e) => {
          e.preventDefault();
          e.stopPropagation();
          await showLinkPreview(link.dest_page, link.dest_y, e.clientX, e.clientY);
        });
      }

      overlayContainer.appendChild(el);
      storedLinks.push({ ...link, el });
    }

    canvasEl.appendChild(overlayContainer);
    linkOverlays.set(pageIndex, storedLinks);
  } catch (err) {
    console.warn(`Failed to load links for page ${pageIndex}:`, err);
  }
}

// ── Link Preview ────────────────────────────────────────────────────

async function showLinkPreview(destPage, destY, mouseX, mouseY) {
  if (!previewTooltip) return;

  const backdrop = container.querySelector("#pdf-link-backdrop");

  previewTooltip.innerHTML = `
    <div class="pdf-preview-body">
      <div class="pdf-spinner"></div>
    </div>
  `;
  previewTooltip.classList.remove("hidden");
  if (backdrop) backdrop.classList.remove("hidden");

  try {
    const result = await getLinkPreview(destPage, destY);
    const body = previewTooltip.querySelector(".pdf-preview-body");
    if (body) {
      const img = document.createElement("img");
      img.className = "pdf-preview-img";
      img.src = `data:image/png;base64,${result.image}`;

      // After the image loads, scroll to center the target
      img.onload = () => {
        const imgHeight = img.offsetHeight;
        const bodyHeight = body.clientHeight;
        const targetY = imgHeight * result.center_ratio;
        // Scroll so targetY is in the center of the visible area
        body.scrollTop = targetY - bodyHeight / 2;
      };

      body.innerHTML = "";
      body.appendChild(img);
    }
  } catch (err) {
    const body = previewTooltip.querySelector(".pdf-preview-body");
    if (body) {
      body.innerHTML = `<div class="pdf-preview-error">Preview failed</div>`;
    }
  }
}

function hidePreview() {
  if (previewTooltip) previewTooltip.classList.add("hidden");
  const backdrop = container?.querySelector("#pdf-link-backdrop");
  if (backdrop) backdrop.classList.add("hidden");
}

function isPreviewVisible() {
  return previewTooltip && !previewTooltip.classList.contains("hidden");
}

// ── Zoom Controls ───────────────────────────────────────────────────

function zoomIn() {
  setScale(Math.min(currentScale + SCALE_STEP, MAX_SCALE));
}

function zoomOut() {
  setScale(Math.max(currentScale - SCALE_STEP, MIN_SCALE));
}

function zoomFitWidth() {
  if (!metadata) return;
  const scrollArea = container.querySelector("#pdf-scroll-area");
  const availableWidth = scrollArea.clientWidth - 80; // Account for padding
  const fitScale = availableWidth / metadata.page_width;
  setScale(Math.max(MIN_SCALE, Math.min(fitScale, MAX_SCALE)));
}

function setScale(newScale) {
  currentScale = Math.round(newScale * 100) / 100;
  updateZoomLabel();
  reRenderAll();
}

function reRenderAll() {
  if (!metadata) return;

  // Update page wrapper sizes and clear rendered state
  pageElements.forEach((wrapper, i) => {
    const scaledWidth = metadata.page_width * currentScale;
    const scaledHeight = metadata.page_height * currentScale;
    wrapper.style.width = `${scaledWidth}px`;
    wrapper.style.height = `${scaledHeight}px`;
    wrapper.removeAttribute("data-rendered-scale");

    const canvas = wrapper.querySelector(".pdf-page-canvas");
    canvas.innerHTML = `<div class="pdf-page-loading"><div class="pdf-spinner"></div></div>`;
  });

  loadingPages.clear();
  linkOverlays.clear();

  // Re-observe to trigger rendering for visible pages
  if (observer) observer.disconnect();
  setupObserver();
}

function updateZoomLabel() {
  const label = container.querySelector("#pdf-zoom-level");
  if (label) label.textContent = `${Math.round(currentScale * 100)}%`;
}

// ── TOC ─────────────────────────────────────────────────────────────

function toggleToc() {
  tocVisible = !tocVisible;
  const panel = container.querySelector("#pdf-toc-panel");
  if (tocVisible) {
    panel.classList.remove("hidden");
  } else {
    panel.classList.add("hidden");
  }
}

function hideToc() {
  tocVisible = false;
  const panel = container?.querySelector("#pdf-toc-panel");
  if (panel) panel.classList.add("hidden");
}

function renderToc(toc) {
  const tree = container.querySelector("#pdf-toc-tree");
  if (!tree) return;
  tree.innerHTML = "";

  if (!toc || toc.length === 0) {
    tree.innerHTML = `<div class="pdf-toc-empty">No table of contents</div>`;
    return;
  }

  const ul = buildTocList(toc, 0);
  tree.appendChild(ul);
}

function buildTocList(entries, level) {
  const ul = document.createElement("ul");
  ul.className = "pdf-toc-list";
  if (level > 0) ul.style.paddingLeft = "16px";

  for (const entry of entries) {
    const li = document.createElement("li");
    li.className = "pdf-toc-item";

    const wrapper = document.createElement("div");
    wrapper.className = "pdf-toc-item-wrapper";

    const hasChildren = entry.children && entry.children.length > 0;

    if (hasChildren) {
      const toggle = document.createElement("span");
      toggle.className = "material-symbols-outlined pdf-toc-toggle";
      toggle.textContent = "expand_more";
      toggle.addEventListener("click", (e) => {
        e.stopPropagation();
        li.classList.toggle("collapsed");
      });
      wrapper.appendChild(toggle);
    } else {
      const spacer = document.createElement("span");
      spacer.className = "pdf-toc-spacer";
      wrapper.appendChild(spacer);
    }

    const link = document.createElement("button");
    link.className = "pdf-toc-link";
    
    const titleSpan = document.createElement("span");
    titleSpan.className = "pdf-toc-title";
    titleSpan.textContent = entry.title;
    link.appendChild(titleSpan);

    if (entry.page_index !== null && entry.page_index !== undefined) {
      const pageLabel = document.createElement("span");
      pageLabel.className = "pdf-toc-page";
      pageLabel.textContent = entry.page_index + 1;
      link.appendChild(pageLabel);
      link.addEventListener("click", () => scrollToPage(entry.page_index));
    }

    wrapper.appendChild(link);
    li.appendChild(wrapper);

    if (hasChildren) {
      const childUl = buildTocList(entry.children, level + 1);
      li.appendChild(childUl);
    }

    ul.appendChild(li);
  }

  return ul;
}

// ── Search ──────────────────────────────────────────────────────────

let searchResults = [];
let searchResultIndex = -1;

function toggleSearch() {
  searchVisible = !searchVisible;
  const bar = container.querySelector("#pdf-search-bar");
  if (searchVisible) {
    bar.classList.remove("hidden");
    container.querySelector("#pdf-search-input").focus();
  } else {
    hideSearch();
  }
}

function hideSearch() {
  searchVisible = false;
  const bar = container?.querySelector("#pdf-search-bar");
  if (bar) bar.classList.add("hidden");
  searchResults = [];
  searchResultIndex = -1;
  const count = container?.querySelector("#pdf-search-count");
  if (count) count.textContent = "";
}

async function handlePdfSearch() {
  const input = container.querySelector("#pdf-search-input");
  const query = input.value.trim();
  const countEl = container.querySelector("#pdf-search-count");

  if (query.length < 2) {
    searchResults = [];
    searchResultIndex = -1;
    countEl.textContent = "";
    return;
  }

  try {
    searchResults = await searchPdf(query);
    searchResultIndex = searchResults.length > 0 ? 0 : -1;
    countEl.textContent = searchResults.length > 0
      ? `${searchResultIndex + 1} / ${searchResults.length}`
      : "No results";

    if (searchResults.length > 0) {
      scrollToPage(searchResults[0].page_index);
    }
  } catch (err) {
    countEl.textContent = "Error";
    console.error("PDF search error:", err);
  }
}

function navigateSearchResult(direction) {
  if (searchResults.length === 0) return;
  searchResultIndex = (searchResultIndex + direction + searchResults.length) % searchResults.length;
  const countEl = container.querySelector("#pdf-search-count");
  countEl.textContent = `${searchResultIndex + 1} / ${searchResults.length}`;
  scrollToPage(searchResults[searchResultIndex].page_index);
}

// ── Navigation ──────────────────────────────────────────────────────

function scrollToPage(pageIndex) {
  const wrapper = pageElements[pageIndex];
  if (!wrapper) return;
  wrapper.scrollIntoView({ behavior: "smooth", block: "start" });
}

function updatePageIndicator() {
  const scrollArea = container.querySelector("#pdf-scroll-area");
  if (!scrollArea || pageElements.length === 0) return;

  const scrollTop = scrollArea.scrollTop;
  const scrollMid = scrollTop + scrollArea.clientHeight / 3;

  let currentPage = 0;
  for (let i = 0; i < pageElements.length; i++) {
    if (pageElements[i].offsetTop <= scrollMid) {
      currentPage = i;
    } else {
      break;
    }
  }

  updatePageInfo(currentPage);
}

function updatePageInfo(pageIndex) {
  const info = container.querySelector("#pdf-page-info");
  if (info && metadata) {
    info.textContent = `${pageIndex + 1} / ${metadata.total_pages}`;
  }
}

// ── Keyboard Handler ────────────────────────────────────────────────

function handleViewerKeydown(e) {
  const mod = e.ctrlKey || e.metaKey;

  // Ctrl+T — Toggle TOC
  if (mod && e.key.toLowerCase() === "t") {
    e.preventDefault();
    e.stopPropagation();
    toggleToc();
    return;
  }

  // Ctrl+F — Search in PDF
  if (mod && e.key.toLowerCase() === "f") {
    e.preventDefault();
    e.stopPropagation();
    toggleSearch();
    return;
  }

  // +/= — Zoom in
  if ((e.key === "+" || e.key === "=") && mod) {
    e.preventDefault();
    zoomIn();
    return;
  }

  // - — Zoom out
  if (e.key === "-" && mod) {
    e.preventDefault();
    zoomOut();
    return;
  }

  // Escape — Close panels (preview first, then search, then TOC)
  if (e.key === "Escape") {
    if (isPreviewVisible()) {
      e.preventDefault();
      e.stopPropagation();
      hidePreview();
      return;
    }
    if (searchVisible) {
      e.preventDefault();
      e.stopPropagation();
      hideSearch();
      return;
    }
    if (tocVisible) {
      e.preventDefault();
      e.stopPropagation();
      hideToc();
      return;
    }
  }
}

// ── Utilities ───────────────────────────────────────────────────────

function debounce(fn, delay) {
  let timeout;
  return function (...args) {
    clearTimeout(timeout);
    timeout = setTimeout(() => fn.apply(this, args), delay);
  };
}
