/**
 * Configurar dataset › Exibição de colunas (ESPEC-INTERFACE §2.3, [manual:
 * def-column-display-settings.htm]): how the dataset's columns are named,
 * ordered, shown and coloured in lists, legends and the time series, with a
 * preview. Display only: OK applies it without reading the file again. It
 * can be kept as this browser's default for new datasets.
 */
import { useState } from 'react';
import type { ColumnDisplayDto } from '../api/generated/ColumnDisplayDto';
import type { ColumnKind } from '../api/generated/ColumnKind';
import type { ColumnSubtype } from '../api/generated/ColumnSubtype';
import type { DatasetDto } from '../api/generated/DatasetDto';
import { columnView, DEFAULT_DISPLAY, MAX_HEIGHT_DECIMALS, storeDefaultDisplay, SUBTYPE_TAG } from '../display';
import { COLUMN_KIND_LABEL, COLUMN_SUBTYPE_LABEL } from '../labels';

const ORDER_LABEL: Record<ColumnDisplayDto['order'], string> = {
  file: 'Como no arquivo',
  type_then_height: 'Por tipo, depois altura',
  height_then_type: 'Por altura, depois tipo',
};
const DECIMAL_CHOICES = Array.from({ length: MAX_HEIGHT_DECIMALS + 1 }, (_, decimals) => decimals);
const STORED = 'Guardado como padrão deste navegador para os próximos datasets.';
const NOT_STORED = 'Este navegador não guarda dados: o padrão não foi salvo.';

interface Props {
  dataset: DatasetDto;
  value: ColumnDisplayDto;
  onChange: (value: ColumnDisplayDto) => void;
}

export function ColumnDisplayTab({ dataset, value, onChange }: Props) {
  const [stored, setStored] = useState('');
  const set = (patch: Partial<ColumnDisplayDto>) => onChange({ ...value, ...patch });
  const kinds = [...new Set(dataset.columns.map((column) => column.kind))];
  const subtypes = (Object.keys(COLUMN_SUBTYPE_LABEL) as ColumnSubtype[]).filter((subtype) => dataset.columns.some((column) => column.subtype === subtype));
  const isHidden = (kind: ColumnKind, subtype: ColumnSubtype) => value.hidden.some((hidden) => hidden.kind === kind && hidden.subtype === subtype);
  const toggle = (kind: ColumnKind, subtype: ColumnSubtype) =>
    set({
      hidden: isHidden(kind, subtype)
        ? value.hidden.filter((hidden) => !(hidden.kind === kind && hidden.subtype === subtype))
        : [...value.hidden, { kind, subtype }],
    });
  const preview = columnView(dataset, value);

  return (
    <div className="properties column-display">
      <fieldset>
        <legend>Nomes e ordem</legend>
        <label className="check">
          <input type="radio" checked={value.naming === 'file'} onChange={() => set({ naming: 'file' })} />
          Como no arquivo
        </label>
        <label className="check">
          <input type="radio" checked={value.naming === 'systematic'} onChange={() => set({ naming: 'systematic' })} />
          Sistemáticos: tipo, altura e estatística (Vel 100m Média)
        </label>
        <label>
          Casas decimais da altura
          <select value={value.heightDecimals} disabled={value.naming !== 'systematic'} onChange={(event) => set({ heightDecimals: Number(event.target.value) })}>
            {DECIMAL_CHOICES.map((decimals) => (
              <option key={decimals}>{decimals}</option>
            ))}
          </select>
        </label>
        <label>
          Ordem
          <select value={value.order} onChange={(event) => set({ order: event.target.value as ColumnDisplayDto['order'] })}>
            {Object.entries(ORDER_LABEL).map(([order, label]) => (
              <option key={order} value={order}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label className="check">
          <input type="checkbox" checked={value.highestFirst} disabled={value.order === 'file'} onChange={(event) => set({ highestFirst: event.target.checked })} />
          Mais altas primeiro
        </label>
        <label className="check">
          <input type="radio" checked={value.colors === 'palette'} onChange={() => set({ colors: 'palette' })} />
          Cores da paleta do gráfico
        </label>
        <label className="check">
          <input type="radio" checked={value.colors === 'by_type'} onChange={() => set({ colors: 'by_type' })} />
          Uma cor por tipo, mais escura quanto mais alto o sensor
        </label>
        <div className="actions">
          <button className="btn" onClick={() => setStored(storeDefaultDisplay(value) ? STORED : NOT_STORED)}>
            Usar como padrão
          </button>
          <button className="btn" onClick={() => onChange(DEFAULT_DISPLAY)}>
            Como no arquivo
          </button>
        </div>
        {stored && <p className="hint">{stored}</p>}
      </fieldset>
      <fieldset>
        <legend>Visibilidade</legend>
        <table className="grid visibility">
          <thead>
            <tr>
              <th>Tipo</th>
              {subtypes.map((subtype) => (
                <th key={subtype} title={COLUMN_SUBTYPE_LABEL[subtype]}>
                  {SUBTYPE_TAG[subtype]}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {kinds.map((kind) => (
              <tr key={kind}>
                <td>{COLUMN_KIND_LABEL[kind]}</td>
                {subtypes.map((subtype) => (
                  <td key={subtype}>
                    {dataset.columns.some((column) => column.kind === kind && column.subtype === subtype) && (
                      <input
                        type="checkbox"
                        aria-label={`Mostrar ${COLUMN_KIND_LABEL[kind]}, ${COLUMN_SUBTYPE_LABEL[subtype]}`}
                        checked={!isHidden(kind, subtype)}
                        onChange={() => toggle(kind, subtype)}
                      />
                    )}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        <p className="hint">Colunas ocultas saem das listas das análises, da legenda e do gráfico da série temporal; continuam no dataset.</p>
      </fieldset>
      <fieldset>
        <legend>Como fica</legend>
        <ol className="column-preview">
          {preview.shown().map((column) => (
            <li key={column.id} title={column.id}>
              {preview.color(column) && <span className="swatch" style={{ background: preview.color(column) }} />} {preview.name(column)}
            </li>
          ))}
        </ol>
      </fieldset>
    </div>
  );
}
