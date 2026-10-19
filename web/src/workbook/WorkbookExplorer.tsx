/**
 * The Workbook Explorer (ESPEC-INTERFACE §1.1, left pane): a mini-bar of
 * dataset commands, the table of datasets (one row each; the check box puts
 * it in the summary graphs, a click selects it, a double click configures
 * it), and the map of their sites below. The table can be folded away,
 * leaving the map the whole pane.
 */
import { useState } from 'react';
import type { DatasetId } from '../api/generated/DatasetId';
import { formatCount, formatNumber, NO_VALUE } from '../format';
import { ICONS } from '../shell/icons';
import { SiteMap, type Site } from './SiteMap';
import { formatDay, formatStep, formatUtcOffset, lastStepMs, primaryColumns, recordDays, recordYears, type Entry } from './model';

/** Decimals of the record length in years. */
const YEARS_DIGITS = 1;
/** Decimals of a sensor height, m. */
const HEIGHT_DIGITS = 1;
/** File types the add button offers. */
const DATA_FILE_TYPES = '.csv,.txt,.tsv,.dat,text/csv,text/plain';

interface Props {
  entries: Entry[];
  selectedId: DatasetId | null;
  /** Whether commands can run (the core is loaded and idle). */
  ready: boolean;
  onSelect: (id: DatasetId) => void;
  onToggleVisible: (id: DatasetId, visible: boolean) => void;
  onAdd: (files: FileList | null) => void;
  onRemove: (id: DatasetId) => void;
  onMove: (id: DatasetId, step: -1 | 1) => void;
  onConfigure: (id: DatasetId) => void;
}

export function WorkbookExplorer({ entries, selectedId, ready, onSelect, onToggleVisible, onAdd, onRemove, onMove, onConfigure }: Props) {
  const [folded, setFolded] = useState(false);
  const position = entries.findIndex((entry) => entry.result.dataset.id === selectedId);
  const hasSelection = position >= 0 && ready;
  const sites: Site[] = entries.flatMap((entry) => {
    const { latitude, longitude, id, name } = entry.result.dataset;
    return latitude != null && longitude != null ? [{ key: id, name, color: entry.color, latitude, longitude }] : [];
  });
  return (
    <section className="explorer" aria-label="Explorador do workbook">
      <div className="minibar">
        <label className={`iconbutton ${ready ? '' : 'disabled'}`} title="Adicionar dataset(s)…">
          {ICONS.addDataset}
          <input
            type="file"
            multiple
            accept={DATA_FILE_TYPES}
            disabled={!ready}
            onChange={(event) => {
              onAdd(event.target.files);
              event.target.value = '';
            }}
          />
        </label>
        <button className="iconbutton" title="Remover dataset" disabled={!hasSelection} onClick={() => onRemove(selectedId!)}>
          {ICONS.removeDataset}
        </button>
        <button className="iconbutton" title="Mover para cima" disabled={!hasSelection || position === 0} onClick={() => onMove(selectedId!, -1)}>
          {ICONS.moveUp}
        </button>
        <button className="iconbutton" title="Mover para baixo" disabled={!hasSelection || position === entries.length - 1} onClick={() => onMove(selectedId!, 1)}>
          {ICONS.moveDown}
        </button>
        <button className="iconbutton" title="Configurar dataset…" disabled={!hasSelection} onClick={() => onConfigure(selectedId!)}>
          {ICONS.configure}
        </button>
        <button className="iconbutton" title="Anexar dados (ainda não disponível)" disabled>
          {ICONS.append}
        </button>
        <button
          className="iconbutton fold"
          title={folded ? 'Mostrar a lista de datasets' : 'Recolher a lista de datasets'}
          aria-expanded={!folded}
          onClick={() => setFolded(!folded)}
        >
          {folded ? '▸' : '▾'}
          {folded && <span className="fold-count">{formatCount(entries.length)} datasets</span>}
        </button>
      </div>
      <div className="datasets tablewrap" hidden={folded}>
        <table className="grid">
          <thead>
            <tr>
              <th aria-label="Visível nos gráficos-resumo" />
              <th aria-label="Tipo" />
              <th>Dataset</th>
              <th>Cor</th>
              <th>Início</th>
              <th>Fim</th>
              <th>Fuso</th>
              <th className="r">Dias</th>
              <th className="r">Anos</th>
              <th className="r">Passo</th>
              <th className="r">Altura</th>
              <th className="r">Colunas</th>
              <th className="r">Passos</th>
              <th className="r">Pontos</th>
              <th>Colunas primárias</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => {
              const { dataset } = entry.result;
              const primary = primaryColumns(dataset);
              const primaryIds = [primary.speed, primary.direction, primary.temperature].flatMap((column) => (column ? [`'${column.id}'`] : []));
              return (
                <tr
                  key={dataset.id}
                  className={dataset.id === selectedId ? 'sel' : ''}
                  onClick={() => onSelect(dataset.id)}
                  onDoubleClick={() => onConfigure(dataset.id)}
                >
                  <td onClick={(event) => event.stopPropagation()}>
                    <input
                      type="checkbox"
                      aria-label={`Mostrar ${dataset.name} nos gráficos-resumo`}
                      checked={entry.visible}
                      onChange={(event) => onToggleVisible(dataset.id, event.target.checked)}
                    />
                  </td>
                  <td className="typeicon" title="Torre de medição">
                    {ICONS.tower}
                  </td>
                  <td className="name">{dataset.name}</td>
                  <td>
                    <span className="swatch" style={{ background: entry.color }} />
                  </td>
                  <td>{formatDay(dataset.startMs)}</td>
                  <td>{formatDay(lastStepMs(dataset))}</td>
                  <td>{formatUtcOffset(dataset.utcOffsetMin)}</td>
                  <td className="r">{formatCount(Math.round(recordDays(dataset)))}</td>
                  <td className="r">{formatNumber(recordYears(dataset), YEARS_DIGITS)}</td>
                  <td className="r">{formatStep(dataset.stepS)}</td>
                  <td className="r">{primary.speed?.heightM == null ? NO_VALUE : `${formatNumber(primary.speed.heightM, HEIGHT_DIGITS)} m`}</td>
                  <td className="r">{dataset.columns.length}</td>
                  <td className="r">{formatCount(dataset.stepCount)}</td>
                  <td className="r">{formatCount(dataset.stepCount * dataset.columns.length)}</td>
                  <td className="primary">{primaryIds.join(', ') || NO_VALUE}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {!entries.length && (
          <p className="empty-hint">
            Adicione um arquivo de dados com o botão + ou arrastando-o para a janela. CSV, TXT ou TSV de logger ou planilha: separador, decimal,
            data e tipo de cada coluna são detectados. Os dados não saem do seu computador.
          </p>
        )}
      </div>
      <SiteMap sites={sites} />
    </section>
  );
}
