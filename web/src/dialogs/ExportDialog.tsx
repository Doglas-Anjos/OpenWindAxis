/** File › Export Data: CSV, metadata JSON, the WAsP observed wind climate and the EPE file of one dataset. */
import { useState } from 'react';
import type { WindApi } from '../api/client';
import type { DatasetId } from '../api/generated/DatasetId';
import { EpeExport } from '../EpeExport';
import { OpenwindExport } from '../OpenwindExport';
import { WaspTabExport } from '../WaspTabExport';
import { baseName, type Entry } from '../workbook/model';
import { download, type Workbook } from '../workbook/useWorkbook';
import { Modal } from './Modal';

interface Props {
  api: WindApi;
  entries: Entry[];
  initialDataset: DatasetId | null;
  workbook: Workbook;
  onClose: () => void;
}

export function ExportDialog({ api, entries, initialDataset, workbook, onClose }: Props) {
  const [chosen, setChosen] = useState<DatasetId | null>(initialDataset);
  const [includeDerived, setIncludeDerived] = useState(false);
  const entry = entries.find((candidate) => candidate.result.dataset.id === chosen) ?? entries[0];
  return (
    <Modal
      title="Exportar dados"
      onClose={onClose}
      wide
      buttons={
        <button className="btn" onClick={onClose}>
          Fechar
        </button>
      }
    >
      {!entry ? (
        <p>Nenhum dataset no workbook.</p>
      ) : (
        <div className="export">
          <label className="dataset-select">
            Dataset
            <select value={entry.result.dataset.id} onChange={(event) => setChosen(Number(event.target.value))}>
              {entries.map((candidate) => (
                <option key={candidate.result.dataset.id} value={candidate.result.dataset.id}>
                  {candidate.result.dataset.name}
                </option>
              ))}
            </select>
          </label>
          <fieldset>
            <legend>Série temporal</legend>
            <label className="check">
              <input type="checkbox" checked={includeDerived} onChange={(event) => setIncludeDerived(event.target.checked)} />
              Incluir as colunas derivadas no CSV
            </label>
            <div className="actions">
              <button className="btn" onClick={() => workbook.exportCsv(entry, ',', includeDerived)}>
                CSV (vírgula, ponto decimal)
              </button>
              <button className="btn" onClick={() => workbook.exportCsv(entry, ';', includeDerived)}>
                CSV para Excel pt-BR
              </button>
              <button className="btn" onClick={() => workbook.exportJson(entry)}>
                Metadados JSON
              </button>
            </div>
          </fieldset>
          <fieldset>
            <legend>Clima de vento observado (WAsP .tab)</legend>
            <WaspTabExport
              key={entry.result.dataset.id}
              api={api}
              dataset={entry.result.dataset}
              fileBase={baseName(entry.file)}
              onDownload={download}
              onError={(message) => workbook.setErrors([message])}
            />
          </fieldset>
          <fieldset>
            <legend>Openwind</legend>
            <OpenwindExport
              key={entry.result.dataset.id}
              api={api}
              dataset={entry.result.dataset}
              sourceName={entry.file.name}
              onDownload={download}
              onError={(message) => workbook.setErrors([message])}
            />
          </fieldset>
          <fieldset>
            <legend>EPE (leilões no Brasil)</legend>
            <EpeExport key={entry.result.dataset.id} api={api} dataset={entry.result.dataset} onDownload={download} onError={(message) => workbook.setErrors([message])} />
          </fieldset>
        </div>
      )}
    </Modal>
  );
}
