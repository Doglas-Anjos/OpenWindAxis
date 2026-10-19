/**
 * An analysis window: the dataset it reads (top of the control column,
 * ESPEC-INTERFACE §3.1) and the "Filtrar por" group (bottom), around one of
 * the analyses of Analyses.tsx. Its dataset starts as the explorer's
 * selection and stays the window's own choice afterwards. Some windows also
 * offer "<todos visíveis>": every visible dataset at once.
 */
import { useMemo, useState } from 'react';
import type { WindApi } from '../api/client';
import type { DatasetId } from '../api/generated/DatasetId';
import { Analysis, WindowSlots, type AnalysisKind, type RecipeRequest } from '../Analyses';
import { FilterPanel, filterRequest } from '../FilterBy';
import { boomsOf, type Entry } from '../workbook/model';
import type { Workbook } from '../workbook/useWorkbook';
import { MultiDatasetAnalysis, MULTI_DATASET_KINDS } from './MultiDatasetAnalysis';

/** Value of the "<todos visíveis>" choice in the dataset list. */
const ALL_VISIBLE = 'all';

interface Props {
  api: WindApi;
  kind: AnalysisKind;
  entries: Entry[];
  initialDataset: DatasetId | null;
  workbook: Workbook;
}

export function AnalysisWindow({ api, kind, entries, initialDataset, workbook }: Props) {
  const [chosen, setChosen] = useState<DatasetId | null>(initialDataset);
  const [allVisible, setAllVisible] = useState(false);
  const offersAll = MULTI_DATASET_KINDS.has(kind);
  const entry = entries.find((candidate) => candidate.result.dataset.id === chosen) ?? entries[0];
  const dataset = entry?.result.dataset;
  // A new filter object only when the panel or the dataset changes.
  const filter = useMemo(() => (entry ? filterRequest(entry.filter, entry.result.dataset, kind) : undefined), [entry?.filter, dataset, kind]);
  if (!entry || !dataset) return <div className="summary-empty">Adicione um dataset para usar esta janela.</div>;
  const slots = {
    dataset: (
      <label className="dataset-select">
        Dataset
        <select
          value={allVisible && offersAll ? ALL_VISIBLE : dataset.id}
          onChange={(event) => {
            setAllVisible(event.target.value === ALL_VISIBLE);
            if (event.target.value !== ALL_VISIBLE) setChosen(Number(event.target.value));
          }}
        >
          {offersAll && <option value={ALL_VISIBLE}>&lt;todos visíveis&gt;</option>}
          {entries.map((candidate) => (
            <option key={candidate.result.dataset.id} value={candidate.result.dataset.id}>
              {candidate.result.dataset.name}
            </option>
          ))}
        </select>
      </label>
    ),
    filter: (
      <FilterPanel
        dataset={dataset}
        kind={kind}
        state={entry.filter}
        onChange={(next) => workbook.updateEntry(dataset.id, () => ({ filter: next }))}
      />
    ),
    exportRecipe: ({ analysis, request }: RecipeRequest) => workbook.exportRecipe(entry, analysis, request),
  };
  if (allVisible && offersAll) {
    // Each dataset brings its own filter; recipes are of one dataset.
    return (
      <WindowSlots.Provider value={{ dataset: slots.dataset }}>
        <MultiDatasetAnalysis api={api} kind={kind} entries={entries} />
      </WindowSlots.Provider>
    );
  }
  return (
    <WindowSlots.Provider value={slots}>
      <Analysis key={`${dataset.id}-${kind}`} kind={kind} api={api} result={entry.result} filter={filter} booms={boomsOf(entry)} />
    </WindowSlots.Provider>
  );
}
