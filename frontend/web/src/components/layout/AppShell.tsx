import { Database, FileText, Folder, LayoutPanelLeft, PanelRight, Plus, Search, Settings } from "lucide-react";

const spaces = ["P", "R", "A", "Z", "D", "W", "B", "I", "O", "T", "L"];
const treeItems = ["daily", "2026-06-12.md", "inbox.md", "projects", "reference-01.md", "reference-02.md"];

export function AppShell() {
  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar />
      <main className="grid min-h-0 flex-1 grid-cols-[56px_300px_minmax(0,1fr)_320px] border-y border-border">
        <ActivityRail />
        <PrimarySidebar />
        <EditorArea />
        <AuxiliarySidebar />
      </main>
      <StatusBar />
    </div>
  );
}

function TitleBar() {
  return (
    <header className="flex h-12 items-center justify-between border-b border-border bg-surface px-3">
      <div className="flex items-center gap-2 font-semibold">
        <div className="grid size-7 place-items-center rounded-lg bg-primary text-sm font-bold text-bg">N</div>
        <span>Notegate</span>
      </div>
      <div className="flex items-center gap-2 text-muted">
        <button className="rounded-md border border-border bg-panel p-1.5" aria-label="Toggle primary sidebar">
          <LayoutPanelLeft size={16} />
        </button>
        <button className="rounded-md border border-border bg-panel p-1.5" aria-label="Add editor group">
          <Plus size={16} />
        </button>
        <button className="rounded-md border border-border bg-panel p-1.5" aria-label="Toggle auxiliary sidebar">
          <PanelRight size={16} />
        </button>
      </div>
    </header>
  );
}

function ActivityRail() {
  return (
    <aside className="flex min-h-0 flex-col border-r border-border bg-surface">
      <div className="flex min-h-0 flex-1 flex-col items-center gap-2 overflow-y-auto py-3">
        {spaces.map((space, index) => (
          <button
            key={space}
            className={`grid size-9 place-items-center rounded-xl border text-sm font-semibold ${
              index === 0 ? "border-primary bg-panel-strong text-text" : "border-border bg-panel text-muted"
            }`}
          >
            {space}
          </button>
        ))}
        <button className="grid size-9 place-items-center rounded-xl border border-dashed border-border text-muted" aria-label="Add space">
          <Plus size={16} />
        </button>
      </div>
      <div className="border-t border-border p-2">
        <button className="grid size-10 place-items-center rounded-xl border border-border bg-panel text-muted" aria-label="Settings">
          <Settings size={16} />
        </button>
      </div>
    </aside>
  );
}

function PrimarySidebar() {
  return (
    <aside className="flex min-h-0 flex-col border-r border-border bg-panel">
      <div className="flex h-12 items-center justify-between border-b border-border px-4">
        <div>
          <div className="text-sm font-semibold">Personal</div>
          <div className="text-xs text-muted">active space</div>
        </div>
        <button className="rounded-md border border-border bg-surface p-1.5 text-muted" aria-label="Create node">
          <Plus size={15} />
        </button>
      </div>
      <div className="grid min-h-0 flex-1 grid-rows-[2fr_6px_1fr]">
        <section className="min-h-0 overflow-y-auto px-3 py-3">
          <SectionTitle icon={<Folder size={13} />} label="Tree" />
          <div className="mt-2 space-y-1">
            <TreeRow name="/" depth={0} folder selected={false} />
            {treeItems.map((item, index) => (
              <TreeRow key={item} name={item} depth={item.includes("2026") || item === "inbox.md" ? 2 : 1} folder={!item.includes(".")} selected={index === 1} />
            ))}
          </div>
        </section>
        <div className="cursor-row-resize border-y border-border bg-surface" />
        <section className="min-h-0 overflow-y-auto px-3 py-3">
          <SectionTitle icon={<Search size={13} />} label="Recent" />
          <div className="mt-2 space-y-1">
            {treeItems.slice(1, 5).map((item) => (
              <TreeRow key={item} name={item} depth={0} folder={!item.includes(".")} selected={item === "2026-06-12.md"} />
            ))}
          </div>
        </section>
      </div>
    </aside>
  );
}

function SectionTitle({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted">
      {icon}
      {label}
    </div>
  );
}

function TreeRow({ name, depth, folder, selected }: { name: string; depth: number; folder: boolean; selected: boolean }) {
  const Icon = folder ? Folder : FileText;
  return (
    <button
      className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${selected ? "bg-panel-strong text-text" : "text-muted hover:bg-surface hover:text-text"}`}
      style={{ paddingLeft: `${8 + depth * 14}px` }}
    >
      <Icon size={15} />
      <span className="truncate">{name}</span>
    </button>
  );
}

function EditorArea() {
  return (
    <section className="min-w-0 bg-bg">
      <div className="flex h-12 items-center justify-between border-b border-border px-4">
        <div className="flex items-center gap-2 font-semibold">
          <FileText size={17} />
          <span>2026-06-12.md</span>
        </div>
        <button className="rounded-md border border-border bg-panel px-3 py-1 text-sm text-muted">Edit</button>
      </div>
      <article className="mx-auto max-w-3xl px-10 py-14">
        <h1 className="text-4xl font-semibold tracking-tight">2026-06-12</h1>
        <h2 className="mt-10 text-2xl font-semibold">Notes</h2>
        <p className="mt-6 leading-7 text-muted">Notegate UI should feel like a focused personal workspace.</p>
        <h2 className="mt-10 text-2xl font-semibold">Decisions</h2>
        <ul className="mt-6 list-disc space-y-3 pl-5 text-muted">
          <li>ActivityRail is a scrollable Space rail.</li>
          <li>PrimarySidebar owns Tree and Recent.</li>
          <li>AuxiliarySidebar starts as Inspector and Agent.</li>
        </ul>
      </article>
    </section>
  );
}

function AuxiliarySidebar() {
  return (
    <aside className="min-h-0 border-l border-border bg-panel p-3">
      <div className="grid grid-cols-2 rounded-lg bg-surface p-1 text-sm">
        <button className="rounded-md bg-panel-strong px-3 py-1.5 font-medium">Inspector</button>
        <button className="rounded-md px-3 py-1.5 text-muted">Agent</button>
      </div>
      <div className="mt-4 space-y-3">
        <InspectorCard title="Node">
          <dl className="grid grid-cols-[80px_1fr] gap-y-2 text-sm">
            <dt className="font-semibold text-text">Kind</dt>
            <dd className="text-muted">text</dd>
            <dt className="font-semibold text-text">Path</dt>
            <dd className="break-all text-muted">/daily/2026-06-12.md</dd>
            <dt className="font-semibold text-text">Updated</dt>
            <dd className="text-muted">2026-06-12</dd>
          </dl>
        </InspectorCard>
        <InspectorCard title="Metadata">
          <pre className="whitespace-pre-wrap font-mono text-xs text-muted">{`{\n  "status": "draft",\n  "tags": ["note"]\n}`}</pre>
        </InspectorCard>
      </div>
    </aside>
  );
}

function InspectorCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-border bg-surface p-4">
      <h3 className="mb-3 text-xs font-bold uppercase tracking-wide text-muted">{title}</h3>
      {children}
    </section>
  );
}

function StatusBar() {
  return (
    <footer className="flex h-7 items-center justify-between border-t border-border bg-surface px-3 text-xs text-muted">
      <span className="flex items-center gap-2"><span className="size-2 rounded-full bg-success" /> saved</span>
      <span>Personal</span>
    </footer>
  );
}
