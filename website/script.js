// ── Footer partial ─────────────────────────────────────────────────
async function loadFooter() {
  const placeholder = document.getElementById('footer-placeholder');
  if (!placeholder) return;
  try {
    const res = await fetch('partials/footer.html');
    const html = await res.text();
    const tmp = document.createElement('div');
    tmp.innerHTML = html.trim();
    placeholder.replaceWith(tmp.firstElementChild);
  } catch (e) { /* no-op in local file:// */ }
}
document.addEventListener('DOMContentLoaded', loadFooter);

// ── Copy install command ───────────────────────────────────────────
function copyInstall(btn) {
  const cmd = btn.closest('.install-box').querySelector('.install-cmd').textContent;
  navigator.clipboard.writeText(cmd).then(() => {
    btn.classList.add('copied');
    btn.innerHTML = '<svg width="15" height="15" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg>';
    setTimeout(() => {
      btn.classList.remove('copied');
      btn.innerHTML = '<svg width="15" height="15" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
    }, 2000);
  });
}

// ── OS tabs ────────────────────────────────────────────────────────
document.querySelectorAll('.os-tab').forEach(btn => {
  btn.addEventListener('click', (e) => {
    e.preventDefault();
    const tabs = btn.closest('.os-tabs');
    // capture where the tab bar sits in the viewport before the swap
    const beforeTop = tabs ? tabs.getBoundingClientRect().top : 0;

    const os = btn.dataset.os;
    document.querySelectorAll('.os-tab').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.os-content').forEach(c => c.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById('os-' + os)?.classList.add('active');

    // restore scroll so the tab bar stays in the same viewport position
    // (prevents iOS Safari from auto-scrolling the tapped button into view)
    if (tabs) {
      const afterTop = tabs.getBoundingClientRect().top;
      const delta = afterTop - beforeTop;
      if (delta) window.scrollBy({ top: delta, behavior: 'instant' });
    }
    btn.blur();
  });
});

// ── Sidebar scroll-spy ─────────────────────────────────────────────
const sections = document.querySelectorAll('.doc-section[id]');
const sidebarLinks = document.querySelectorAll('.sidebar-link[data-section]');

const spy = new IntersectionObserver(entries => {
  entries.forEach(entry => {
    if (entry.isIntersecting) {
      sidebarLinks.forEach(l => l.classList.remove('active'));
      const link = document.querySelector(`.sidebar-link[data-section="${entry.target.id}"]`);
      if (link) {
        link.classList.add('active');
        // on mobile scroll sidebar to show active link
        link.scrollIntoView({ block: 'nearest', inline: 'center' });
      }
    }
  });
}, { rootMargin: '-20% 0px -70% 0px', threshold: 0 });

sections.forEach(s => spy.observe(s));

// ── Scroll-reveal ──────────────────────────────────────────────────
const revealObserver = new IntersectionObserver(
  entries => entries.forEach(e => { if (e.isIntersecting) e.target.classList.add('visible'); }),
  { threshold: 0.08 }
);
document.querySelectorAll('.feature-row-item, .stat-card, .provider-card, .callout, .comparison-table-wrap, .video-wrapper')
  .forEach(el => { el.classList.add('reveal'); revealObserver.observe(el); });
