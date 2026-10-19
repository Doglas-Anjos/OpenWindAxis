/**
 * The main window (ESPEC-INTERFACE §1): title bar, menu bar, toolbar, the
 * work area split into the Workbook Explorer (left) and the windows (right,
 * as tabs: the summary graphs first and fixed, then the analysis and flag
 * windows opened from the menus), and the status bar. Data files dropped on
 * the window are added as datasets; a workbook file dropped is opened.
 */
import { useEffect, useMemo, useRef, useState, type DragEvent, type PointerEvent as ReactPointerEvent } from 'react';
import type { AnalysisKind } from './Analyses';
import type { DatasetId } from './api/generated/DatasetId';
import { AboutDialog } from './dialogs/AboutDialog';
import { ConfigureDatasetDialog } from './dialogs/ConfigureDatasetDialog';
import { ExportDialog } from './dialogs/ExportDialog';
import { ExtrapolationDialog } from './dialogs/ExtrapolationDialog';
import { WorkflowDialog } from './dialogs/WorkflowDialog';
import { DefineFlagsDialog } from './dialogs/DefineFlagsDialog';
import { Modal } from './dialogs/Modal';
import { AppendStatusDialog } from './dialogs/AppendStatusDialog';
import { ReviseDataDialog, type ReviseKind } from './dialogs/ReviseDataDialog';
import { FlagRulesCard } from './FlagRules';
import { TowerShadingWindow } from './TowerShading';
import { FlagScatterCard } from './FlagScatter';
import { FilterPanel, filterRequest } from './FilterBy';
import { FlagStatisticsCard } from './FlagStatistics';
import { DisplayContext } from './display';
import { PROJECT_EXTENSION } from './project';
import { PRODUCT_NAME } from './shell/brand';
import { ICONS, Logo } from './shell/icons';
import { MenuBar, SEPARATOR, type Menu } from './shell/MenuBar';
import { Toolbar, type ToolbarButton } from './shell/Toolbar';
import { SummaryGraphs } from './summary/SummaryGraphs';
import { AnalysisWindow } from './windows/AnalysisWindow';
import { LongTermWindow } from './windows/LongTermWindow';
import { TimeSeriesWindow } from './windows/TimeSeriesWindow';
import { WorkbookExplorer } from './workbook/WorkbookExplorer';
import { useWorkbook } from './workbook/useWorkbook';

/** How a recent entry's time is shown: the viewer's clock, in pt-BR. */
const LOCALE = 'pt-BR';
const OPENED_AT_FORMAT: Intl.DateTimeFormatOptions = { dateStyle: 'short', timeStyle: 'short' };

/** The windows of the right pane. */
type WindowKind = 'summary' | 'series' | 'flag-manual' | 'flag-scatter' | 'flag-rules' | 'flag-shading' | 'flag-inspect' | 'long-term' | AnalysisKind;

const WINDOW_TITLE: Record<WindowKind, string> = {
  summary: 'Gráficos-resumo',
  series: 'Série temporal',
  'flag-manual': 'Flag manual',
  'flag-rules': 'Flag por regras',
  'flag-shading': 'Sombra da torre',
  'flag-scatter': 'Flag por dispersão',
  'flag-inspect': 'Inspecionar e remover flags',
  'long-term': 'Ajuste de longo prazo',
  coverage: 'Cobertura de dados',
  histogram: 'Histograma',
  rose: 'Rosa dos ventos',
  diurnal: 'Perfil diurno',
  shear: 'Cisalhamento do vento',
  turbulence: 'Turbulência',
  distribution: 'Distribuição de velocidade',
  distortion: 'Distorção da torre',
  extremes: 'Ventos extremos',
  scatter: 'Dispersão',
  tables: 'Tabelas',
};
const ANALYSIS_KINDS: AnalysisKind[] = ['coverage', 'histogram', 'distribution', 'rose', 'diurnal', 'shear', 'turbulence', 'distortion', 'extremes', 'scatter', 'tables'];
const isAnalysis = (kind: WindowKind): kind is AnalysisKind => (ANALYSIS_KINDS as string[]).includes(kind);

type Dialog =
  | { kind: 'configure'; dataset: DatasetId }
  | { kind: 'revise'; revise: ReviseKind; dataset: DatasetId }
  | { kind: 'extrapolation'; dataset: DatasetId }
  | { kind: 'workflow'; dataset: DatasetId }
  | { kind: 'flags'; dataset: DatasetId }
  | { kind: 'export' }
  | { kind: 'about' }
  | { kind: 'new' }
  | null;

/** Explorer width: at start, and the limits the divider keeps, px. */
const EXPLORER_START_PX = 640;
const EXPLORER_MIN_PX = 280;
const RIGHT_MIN_PX = 420;
/** Keys that act with Ctrl (⌘ on a Mac) outside text fields. */
const UNDO_KEY = 'z';
const REDO_KEY = 'y';
const OPEN_KEY = 'o';
const SAVE_KEY = 's';
/** Opens the help, as the original's F1. */
const HELP_KEY = 'F1';
/** The help pages (web/public/ajuda, built by docs/ajuda), in a tab of their own that F1 reuses. */
const HELP_URL = `${import.meta.env.BASE_URL}ajuda/index.html`;
const HELP_TARGET = 'openwindaxis-ajuda';
const openHelp = () => window.open(HELP_URL, HELP_TARGET, 'noopener');
/** Elements that keep Ctrl+Z for their own text editing. */
const TEXT_EDITING_TAGS = new Set(['INPUT', 'TEXTAREA', 'SELECT']);
/** File types the add-dataset command offers. */
const DATA_FILE_TYPES = '.csv,.txt,.tsv,.dat,text/csv,text/plain';
/** Extensions of the data files a folder pick keeps; the rest of the folder is left alone. */
const DATA_FILE_EXTENSION = /\.(csv|txt|tsv|dat)$/i;

/** The data files of a folder pick, without hidden files. */
const dataFiles = (files: FileList | null) => Array.from(files ?? []).filter((file) => DATA_FILE_EXTENSION.test(file.name) && !file.name.startsWith('.'));

export default function App() {
  const workbook = useWorkbook();
  const { api, entries, selected, selectedId, busy, errors } = workbook;
  /** Each dataset's column display settings, for whatever lists its columns. */
  const displays = useMemo(() => new Map(entries.map((entry) => [entry.result.dataset.id, entry.display])), [entries]);
  const ready = !!api && !busy;
  const [windows, setWindows] = useState<WindowKind[]>(['summary']);
  const [active, setActive] = useState<WindowKind>('summary');
  const [dialog, setDialog] = useState<Dialog>(null);
  const [explorerShown, setExplorerShown] = useState(true);
  const [explorerWidth, setExplorerWidth] = useState(EXPLORER_START_PX);
  const openInput = useRef<HTMLInputElement>(null);
  const addInput = useRef<HTMLInputElement>(null);
  const appendInput = useRef<HTMLInputElement>(null);
  const appendFolderInput = useRef<HTMLInputElement>(null);
  const openFolderInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    document.title = `${workbook.workbookName} - ${PRODUCT_NAME}`;
  }, [workbook.workbookName]);

  const openWindow = (kind: WindowKind) => {
    setWindows((current) => (current.includes(kind) ? current : [...current, kind]));
    setActive(kind);
  };
  const closeWindow = (kind: WindowKind) => {
    if (kind === 'summary') return;
    setWindows((current) => current.filter((open) => open !== kind));
    setActive((current) => (current === kind ? 'summary' : current));
  };
  const configure = (id: DatasetId | null) => id != null && setDialog({ kind: 'configure', dataset: id });
  const revise = (kind: ReviseKind) => selectedId != null && setDialog({ kind: 'revise', revise: kind, dataset: selectedId });
  const undo = () => selected && selected.canUndo && ready && workbook.stepHistory(selected, 'undo');
  const redo = () => selected && selected.canRedo && ready && workbook.stepHistory(selected, 'redo');

  // Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y undo and redo edits; Ctrl+O opens a
  // workbook and Ctrl+S saves it. Text fields keep their own Ctrl+Z.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      if (event.key === HELP_KEY) {
        event.preventDefault();
        openHelp();
        return;
      }
      if (!(event.ctrlKey || event.metaKey) || TEXT_EDITING_TAGS.has(target.tagName) || target.isContentEditable) return;
      const key = event.key.toLowerCase();
      const command =
        key === REDO_KEY || (key === UNDO_KEY && event.shiftKey)
          ? redo
          : key === UNDO_KEY
            ? undo
            : key === OPEN_KEY
              ? () => ready && openInput.current?.click()
              : key === SAVE_KEY
                ? () => ready && workbook.saveWorkbook()
                : null;
      if (!command) return;
      event.preventDefault();
      void command();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  /** Files dropped on the window: workbooks open, data files become datasets. */
  const onDrop = (event: DragEvent) => {
    event.preventDefault();
    if (!ready) return;
    const files = Array.from(event.dataTransfer.files);
    const workbooks = files.filter((file) => file.name.toLowerCase().endsWith(PROJECT_EXTENSION));
    const data = files.filter((file) => !workbooks.includes(file));
    if (workbooks.length) void workbook.openWorkbook(workbooks[0]);
    if (data.length) void workbook.importFiles(data);
  };

  /** Drags the divider between the explorer and the windows. */
  const startResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    const startX = event.clientX;
    const startWidth = explorerWidth;
    const move = (moved: PointerEvent) => {
      const limit = window.innerWidth - RIGHT_MIN_PX;
      setExplorerWidth(Math.min(limit, Math.max(EXPLORER_MIN_PX, startWidth + moved.clientX - startX)));
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  };

  const noDataset = !ready || !selected;
  const menus: Menu[] = [
    {
      label: 'Arquivo',
      items: [
        { label: 'Novo workbook', action: () => (entries.length ? setDialog({ kind: 'new' }) : workbook.newWorkbook()), disabled: !ready },
        { label: 'Abrir workbook…', action: () => openInput.current?.click(), disabled: !ready, shortcut: 'Ctrl+O' },
        { label: 'Adicionar dataset(s)…', action: () => addInput.current?.click(), disabled: !ready },
        { label: 'Abrir pasta como dataset…', action: () => openFolderInput.current?.click(), disabled: !ready },
        { label: 'Anexar dados…', action: () => appendInput.current?.click(), disabled: noDataset },
        { label: 'Anexar pasta…', action: () => appendFolderInput.current?.click(), disabled: noDataset },
        SEPARATOR,
        { label: 'Salvar workbook', action: () => workbook.saveWorkbook(), disabled: !ready || !entries.length, shortcut: 'Ctrl+S' },
        { label: 'Salvar workbook como…', action: () => workbook.saveWorkbook(), disabled: !ready || !entries.length },
        SEPARATOR,
        { label: 'Exportar dados…', action: () => setDialog({ kind: 'export' }), disabled: !ready || !entries.length },
        SEPARATOR,
        ...(workbook.recent.length
          ? workbook.recent.map((item, index) => ({
              label: `${index + 1}. ${item.name}`,
              action: () => workbook.openRecent(item.id),
              disabled: !ready,
              title: `${item.kind === 'workbook' ? 'Workbook' : 'Arquivo de dados'} aberto em ${new Date(item.savedAt).toLocaleString(LOCALE, OPENED_AT_FORMAT)} (guardado no navegador)`,
            }))
          : [{ label: 'Abertos recentemente: nenhum', disabled: true, title: 'Os workbooks e arquivos abertos aparecem aqui, guardados no navegador.' }]),
      ],
    },
    {
      label: 'Editar',
      items: [
        { label: 'Desfazer', action: undo, disabled: noDataset || !selected?.canUndo, shortcut: 'Ctrl+Z' },
        { label: 'Refazer', action: redo, disabled: noDataset || !selected?.canRedo, shortcut: 'Ctrl+Y' },
      ],
    },
    {
      label: 'Exibir',
      items: [
        { label: 'Explorador do workbook', action: () => setExplorerShown(!explorerShown), checked: explorerShown },
        { label: 'Gráficos-resumo', action: () => setActive('summary') },
        SEPARATOR,
        { label: 'Histórico do dataset' },
        { label: 'Relatórios' },
        { label: 'Assistente' },
      ],
    },
    {
      label: 'Revisar',
      items: [
        { label: 'Configurar dataset…', action: () => configure(selectedId), disabled: noDataset },
        SEPARATOR,
        { label: 'Excluir dados…', action: () => revise('delete'), disabled: noDataset },
        { label: 'Escala e offset…', action: () => revise('scale'), disabled: noDataset },
        { label: 'Deslocar tempo…', action: () => revise('shift'), disabled: noDataset },
        { label: 'Reconstruir lacunas…', action: () => revise('reconstruct'), disabled: noDataset },
        SEPARATOR,
        { label: 'Extrapolação vertical…', action: () => selectedId != null && setDialog({ kind: 'extrapolation', dataset: selectedId }), disabled: noDataset },
        { label: 'Ajuste de longo prazo…', action: () => openWindow('long-term'), disabled: noDataset },
      ],
    },
    {
      label: 'Flag',
      items: [
        { label: 'Flag manual…', action: () => openWindow('flag-manual'), disabled: noDataset },
        { label: 'Flag por dispersão…', action: () => openWindow('flag-scatter'), disabled: noDataset },
        { label: 'Flag por regras…', action: () => openWindow('flag-rules'), disabled: noDataset },
        { label: 'Sombra da torre…', action: () => openWindow('flag-shading'), disabled: noDataset },
        SEPARATOR,
        { label: 'Inspecionar e remover flags…', action: () => openWindow('flag-inspect'), disabled: noDataset },
        { label: 'Definir flags…', action: () => selectedId != null && setDialog({ kind: 'flags', dataset: selectedId }), disabled: noDataset },
        { label: 'Flags favoritas…' },
      ],
    },
    {
      label: 'Analisar',
      items: [
        { label: 'Série temporal', action: () => openWindow('series'), disabled: noDataset },
        { label: 'Dispersão', action: () => openWindow('scatter'), disabled: noDataset },
        { label: 'Histograma', action: () => openWindow('histogram'), disabled: noDataset },
        { label: 'Perfil diurno', action: () => openWindow('diurnal'), disabled: noDataset },
        { label: 'Rosa dos ventos', action: () => openWindow('rose'), disabled: noDataset },
        { label: 'Cobertura de dados', action: () => openWindow('coverage'), disabled: noDataset },
        { label: 'Turbulência', action: () => openWindow('turbulence'), disabled: noDataset },
        { label: 'Cisalhamento do vento', action: () => openWindow('shear'), disabled: noDataset },
        { label: 'Distribuição de velocidade', action: () => openWindow('distribution'), disabled: noDataset },
        { label: 'Distorção da torre', action: () => openWindow('distortion'), disabled: noDataset },
        { label: 'Ventos extremos', action: () => openWindow('extremes'), disabled: noDataset },
        { label: 'Mapa de dados (DMap)' },
        { label: 'Tabelas', action: () => openWindow('tables'), disabled: noDataset },
      ],
    },
    {
      label: 'Fluxos',
      items: [
        { label: 'Gerenciar fluxos…', action: () => selectedId != null && setDialog({ kind: 'workflow', dataset: selectedId }), disabled: noDataset },
        { label: 'Executar fluxo', action: () => selectedId != null && setDialog({ kind: 'workflow', dataset: selectedId }), disabled: noDataset },
      ],
    },
    { label: 'Ferramentas', items: [{ label: 'Opções…' }] },
    {
      label: 'Janela',
      items: [
        { label: 'Fechar janela', action: () => closeWindow(active), disabled: active === 'summary' },
        { label: 'Fechar todas as janelas', action: () => (setWindows(['summary']), setActive('summary')), disabled: windows.length === 1 },
        SEPARATOR,
        ...windows.map((kind) => ({ label: WINDOW_TITLE[kind], action: () => setActive(kind), checked: kind === active })),
      ],
    },
    {
      label: 'Ajuda',
      items: [{ label: 'Conteúdo da ajuda', action: openHelp, shortcut: HELP_KEY }, SEPARATOR, { label: `Sobre o ${PRODUCT_NAME}…`, action: () => setDialog({ kind: 'about' }) }],
    },
  ];

  const toolbar: ToolbarButton[][] = [
    [
      { icon: ICONS.newFile, label: 'Novo workbook', action: () => (entries.length ? setDialog({ kind: 'new' }) : workbook.newWorkbook()), disabled: !ready },
      { icon: ICONS.open, label: 'Abrir workbook… (Ctrl+O)', action: () => openInput.current?.click(), disabled: !ready },
      { icon: ICONS.save, label: 'Salvar workbook (Ctrl+S)', action: () => workbook.saveWorkbook(), disabled: !ready || !entries.length },
    ],
    [{ icon: ICONS.configure, label: 'Configurar dataset…', action: () => configure(selectedId), disabled: noDataset }],
    [
      { icon: ICONS.flagManual, label: 'Flag manual', action: () => openWindow('flag-manual'), disabled: noDataset },
      { icon: ICONS.flagRules, label: 'Flag por regras', action: () => openWindow('flag-rules'), disabled: noDataset },
      { icon: ICONS.inspectFlags, label: 'Inspecionar e remover flags', action: () => openWindow('flag-inspect'), disabled: noDataset },
    ],
    [
      { icon: ICONS.timeSeries, label: 'Série temporal', action: () => openWindow('series'), disabled: noDataset },
      { icon: ICONS.histogram, label: 'Histograma', action: () => openWindow('histogram'), disabled: noDataset },
      { icon: ICONS.windRose, label: 'Rosa dos ventos', action: () => openWindow('rose'), disabled: noDataset },
      { icon: ICONS.diurnal, label: 'Perfil diurno', action: () => openWindow('diurnal'), disabled: noDataset },
      { icon: ICONS.coverage, label: 'Cobertura de dados', action: () => openWindow('coverage'), disabled: noDataset },
      { icon: ICONS.shear, label: 'Cisalhamento do vento', action: () => openWindow('shear'), disabled: noDataset },
      { icon: ICONS.turbulence, label: 'Turbulência', action: () => openWindow('turbulence'), disabled: noDataset },
    ],
    [
      { icon: ICONS.undo, label: 'Desfazer (Ctrl+Z)', action: undo, disabled: noDataset || !selected?.canUndo },
      { icon: ICONS.redo, label: 'Refazer (Ctrl+Y)', action: redo, disabled: noDataset || !selected?.canRedo },
    ],
    [{ icon: ICONS.help, label: `Sobre o ${PRODUCT_NAME}`, action: () => setDialog({ kind: 'about' }) }],
  ];

  /** The content of one window of the right pane. */
  const windowContent = (kind: WindowKind) => {
    if (!api) return null;
    if (kind === 'summary') return <SummaryGraphs api={api} entries={entries} />;
    if (isAnalysis(kind)) return <AnalysisWindow api={api} kind={kind} entries={entries} initialDataset={selectedId} workbook={workbook} />;
    if (kind === 'long-term') return <LongTermWindow api={api} entries={entries} initialDataset={selectedId} workbook={workbook} />;
    if (!selected) return <div className="summary-empty">Selecione um dataset no explorador.</div>;
    const dataset = selected.result.dataset;
    switch (kind) {
      case 'series':
      case 'flag-manual':
        return <TimeSeriesWindow key={dataset.id} api={api} entry={selected} workbook={workbook} flagging={kind === 'flag-manual'} ready={ready} />;
      case 'flag-rules':
        return (
          <FlagRulesCard
            key={dataset.id}
            api={api}
            dataset={dataset}
            rules={selected.rules}
            onRulesChange={(rules) => workbook.updateEntry(dataset.id, () => ({ rules }))}
            onEdit={(edited) => workbook.showEdit(selected, edited)}
            disabled={!ready}
          />
        );
      case 'flag-scatter':
        return (
          <div className="flagscatter-window">
            <FlagScatterCard
              key={dataset.id}
              api={api}
              dataset={dataset}
              filter={filterRequest(selected.filter, dataset, 'scatter')}
              onApply={(op) => workbook.applyOp(selected, op)}
              disabled={!ready}
            />
            <FilterPanel dataset={dataset} kind="scatter" state={selected.filter} onChange={(next) => workbook.updateEntry(dataset.id, () => ({ filter: next }))} />
          </div>
        );
      case 'flag-shading':
        return (
          <TowerShadingWindow api={api} entries={entries} initialDataset={dataset.id} workbook={workbook} disabled={!ready} />
        );
      case 'flag-inspect':
        return <FlagStatisticsCard key={dataset.id} api={api} dataset={dataset} onEdit={(edited) => workbook.showEdit(selected, edited)} disabled={!ready} startOpen />;
    }
  };

  return (
    <DisplayContext.Provider value={displays}>
      <div className="mainwindow" onDragOver={(event) => event.preventDefault()} onDrop={onDrop}>
      <div className="titlebar">
        <Logo />
        <span>
          {workbook.workbookName} - {PRODUCT_NAME}
        </span>
      </div>
      <MenuBar menus={menus} />
      <Toolbar groups={toolbar} />
      <input ref={openInput} type="file" hidden accept={PROJECT_EXTENSION} onChange={(event) => (workbook.openWorkbook(event.target.files?.[0]), (event.target.value = ''))} />
      <input ref={addInput} type="file" hidden multiple accept={DATA_FILE_TYPES} onChange={(event) => (workbook.importFiles(event.target.files), (event.target.value = ''))} />
      <input
        ref={appendInput}
        type="file"
        hidden
        multiple
        accept={DATA_FILE_TYPES}
        onChange={(event) => (selectedId != null && workbook.appendFiles(selectedId, event.target.files), (event.target.value = ''))}
      />
      {/* webkitdirectory picks a whole folder; React has no typed prop for it. */}
      <input
        ref={appendFolderInput}
        type="file"
        hidden
        {...{ webkitdirectory: '' }}
        onChange={(event) => (selectedId != null && workbook.appendFiles(selectedId, dataFiles(event.target.files)), (event.target.value = ''))}
      />
      <input
        ref={openFolderInput}
        type="file"
        hidden
        {...{ webkitdirectory: '' }}
        onChange={(event) => (workbook.importFolder(dataFiles(event.target.files)), (event.target.value = ''))}
      />
      {errors.length > 0 && (
        <div className="errorbar" role="alert">
          <div>
            {errors.map((message, index) => (
              <div key={index}>{message}</div>
            ))}
          </div>
          <button className="iconbutton" aria-label="Dispensar" onClick={() => workbook.setErrors([])}>
            {ICONS.close}
          </button>
        </div>
      )}
      <div className="workarea">
        {explorerShown && (
          <>
            <div className="pane left" style={{ width: explorerWidth }}>
              <WorkbookExplorer
                entries={entries}
                selectedId={selectedId}
                ready={ready}
                onSelect={workbook.setSelectedId}
                onToggleVisible={(id, visible) => workbook.updateEntry(id, () => ({ visible }))}
                onAdd={workbook.importFiles}
                onRemove={workbook.remove}
                onMove={workbook.move}
                onConfigure={configure}
              />
            </div>
            <div className="divider" role="separator" aria-orientation="vertical" onPointerDown={startResize} />
          </>
        )}
        <div className="pane right">
          <div className="windowtabs" role="tablist">
            {windows.map((kind) => (
              <div key={kind} role="tab" aria-selected={kind === active} className={`windowtab ${kind === active ? 'on' : ''}`} onClick={() => setActive(kind)}>
                <span>{WINDOW_TITLE[kind]}</span>
                {kind !== 'summary' && (
                  <button
                    className="iconbutton"
                    aria-label={`Fechar ${WINDOW_TITLE[kind]}`}
                    onClick={(event) => {
                      event.stopPropagation();
                      closeWindow(kind);
                    }}
                  >
                    {ICONS.close}
                  </button>
                )}
              </div>
            ))}
          </div>
          <div className="windows">
            {windows.map((kind) => (
              // Open windows stay mounted, so switching tabs keeps their state;
              // the ones behind keep their size, so their charts draw at the right size.
              <div key={kind} className={`window ${kind === active ? '' : 'behind'}`} aria-hidden={kind !== active}>
                {windowContent(kind)}
              </div>
            ))}
          </div>
        </div>
      </div>
      <div className="statusbar">
        <span>Painel 1</span>
        <span className="status-message">{!api ? 'Carregando o núcleo…' : busy || (selected ? `${selected.result.dataset.name} selecionado` : 'Nenhum dataset')}</span>
        <span title="O assistente de conselhos ainda não está disponível">Conselhos (0)</span>
      </div>

      {dialog?.kind === 'configure' && api && (
        <ConfigureDatasetDialog api={api} entries={entries} initialDataset={dialog.dataset} workbook={workbook} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'revise' && (
        <ReviseDataDialog kind={dialog.revise} entries={entries} initialDataset={dialog.dataset} workbook={workbook} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'extrapolation' && (
        <ExtrapolationDialog entries={entries} initialDataset={dialog.dataset} workbook={workbook} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'flags' && api && entries.some((entry) => entry.result.dataset.id === dialog.dataset) && (
        <DefineFlagsDialog api={api} entry={entries.find((entry) => entry.result.dataset.id === dialog.dataset)!} workbook={workbook} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'workflow' && api && (
        <WorkflowDialog api={api} entries={entries} initialDataset={dialog.dataset} workbook={workbook} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'export' && api && (
        <ExportDialog api={api} entries={entries} initialDataset={selectedId} workbook={workbook} onClose={() => setDialog(null)} />
      )}
      {dialog?.kind === 'about' && <AboutDialog onClose={() => setDialog(null)} />}
      {workbook.appendReport && <AppendStatusDialog report={workbook.appendReport} onClose={() => workbook.setAppendReport(null)} />}
      {dialog?.kind === 'new' && (
        <Modal
          title="Novo workbook"
          onClose={() => setDialog(null)}
          buttons={
            <>
              <button
                className="btn primary"
                onClick={() => {
                  setDialog(null);
                  void workbook.newWorkbook();
                }}
              >
                Descartar e começar
              </button>
              <button className="btn" onClick={() => setDialog(null)}>
                Cancelar
              </button>
            </>
          }
        >
          <p>Os datasets abertos saem da sessão. Salve o workbook antes, se quiser guardar as marcações.</p>
        </Modal>
      )}
      </div>
    </DisplayContext.Provider>
  );
}
