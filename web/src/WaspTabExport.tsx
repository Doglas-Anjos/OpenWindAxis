/**
 * The WAsP `.tab` part of the export card: the observed wind climate of one
 * speed and one direction column (layout in docs/formatos/wasp-tab.md).
 * Latitude and longitude are required, because WAsP uses the latitude and
 * the core never guesses it; an empty height takes the speed column's.
 */
import { useState } from 'react';
import type { WindApi } from './api/client';
import type { ColumnDto } from './api/generated/ColumnDto';
import type { DatasetDto } from './api/generated/DatasetDto';
import { errorMessage, formatNumber } from './format';
import { coordinateText, isMeanDirection, isMeanSpeed } from './workbook/model';

/** Sector counts offered; 12 is WAsP's usual. */
const SECTOR_CHOICES = [8, 12, 16, 36];
const DEFAULT_SECTORS = 12;
/** Width of the speed classes, m/s. */
const DEFAULT_CLASS_WIDTH_M_S = 1;
/** File name extension and media type of the download. */
const TAB_EXTENSION = '.tab';
const TAB_MEDIA_TYPE = 'text/plain';

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** Download name without extension. */
  fileBase: string;
  onDownload: (name: string, contents: Blob) => void;
  onError: (message: string) => void;
}


/** The highest mean speed column, the one nearest hub height; columns without a height come last. */
function highestSpeed(speeds: ColumnDto[]): string | null {
  const height = (column: ColumnDto) => column.heightM ?? -Infinity;
  return speeds.reduce<ColumnDto | null>((best, column) => (best == null || height(column) > height(best) ? column : best), null)?.id ?? null;
}

/** A number typed in a field, or `undefined` when it is empty. */
const typedNumber = (text: string) => (text.trim() === '' ? undefined : Number(text));

/** Form and button that download a WAsP `.tab` of the dataset. */
export function WaspTabExport({ api, dataset, fileBase, onDownload, onError }: Props) {
  const speeds = dataset.columns.filter(isMeanSpeed);
  const directions = dataset.columns.filter(isMeanDirection);
  const [speedId, setSpeedId] = useState(() => highestSpeed(speeds));
  const [directionId, setDirectionId] = useState(() => directions[0]?.id ?? null);
  const [sectors, setSectors] = useState(DEFAULT_SECTORS);
  const [classWidth, setClassWidth] = useState(DEFAULT_CLASS_WIDTH_M_S);
  const [firstClassHalf, setFirstClassHalf] = useState(false);
  // The dataset's site (from the logger preamble or Configurar dataset) fills the header.
  const [latitude, setLatitude] = useState(() => coordinateText(dataset.latitude));
  const [longitude, setLongitude] = useState(() => coordinateText(dataset.longitude));
  const [height, setHeight] = useState('');
  const [busy, setBusy] = useState(false);
  if (speedId == null || directionId == null) {
    return <p className="hint">O WAsP .tab precisa de uma coluna de velocidade média e de uma de direção.</p>;
  }
  const speedHeight = speeds.find((column) => column.id === speedId)?.heightM;
  const located = typedNumber(latitude) !== undefined && typedNumber(longitude) !== undefined;

  async function exportTab() {
    setBusy(true);
    try {
      const text = await api.exportWaspTab({
        dataset: dataset.id,
        speed: speedId!,
        direction: directionId!,
        sectors,
        binWidthMS: classWidth,
        firstBinHalf: firstClassHalf,
        latitudeDeg: typedNumber(latitude),
        longitudeDeg: typedNumber(longitude),
        heightM: typedNumber(height),
      });
      onDownload(`${fileBase}${TAB_EXTENSION}`, new Blob([text], { type: TAB_MEDIA_TYPE }));
    } catch (thrown) {
      onError(errorMessage(thrown));
    }
    setBusy(false);
  }

  return (
    <>
      <h3 className="hint">Clima de vento observado para o WAsP</h3>
      <div className="controls">
        <label>
          Velocidade
          <select value={speedId} onChange={(event) => setSpeedId(event.target.value)}>
            {speeds.map((column) => (
              <option key={column.id} value={column.id}>
                {column.id}
              </option>
            ))}
          </select>
        </label>
        <label>
          Direção
          <select value={directionId} onChange={(event) => setDirectionId(event.target.value)}>
            {directions.map((column) => (
              <option key={column.id} value={column.id}>
                {column.id}
              </option>
            ))}
          </select>
        </label>
        <label>
          Setores
          <select value={sectors} onChange={(event) => setSectors(Number(event.target.value))}>
            {SECTOR_CHOICES.map((choice) => (
              <option key={choice}>{choice}</option>
            ))}
          </select>
        </label>
        <label>
          Classe (m/s)
          <input type="number" step="any" min="0" value={classWidth} onChange={(event) => setClassWidth(Number(event.target.value))} />
        </label>
        <label className="check">
          <input type="checkbox" checked={firstClassHalf} onChange={(event) => setFirstClassHalf(event.target.checked)} />
          1ª classe com meia largura
        </label>
        <label>
          Latitude (°)
          <input type="number" step="any" min="-90" max="90" value={latitude} onChange={(event) => setLatitude(event.target.value)} />
        </label>
        <label>
          Longitude (°)
          <input type="number" step="any" min="-180" max="180" value={longitude} onChange={(event) => setLongitude(event.target.value)} />
        </label>
        <label>
          Altura (m)
          <input
            type="number"
            step="any"
            min="0"
            value={height}
            placeholder={speedHeight == null ? '' : formatNumber(speedHeight)}
            onChange={(event) => setHeight(event.target.value)}
          />
        </label>
        <button
          className="btn"
          onClick={exportTab}
          disabled={!located || busy}
          title={located ? 'Baixar o arquivo .tab' : 'Informe a latitude e a longitude do local'}
        >
          WAsP .tab
        </button>
      </div>
      <p className="hint">
        Frequência por setor (%) e por classe de velocidade dentro de cada setor (‰), com os mesmos passos da rosa dos
        ventos. Latitude e longitude são obrigatórias porque o WAsP usa a latitude; a altura vazia usa a da coluna.
      </p>
    </>
  );
}
