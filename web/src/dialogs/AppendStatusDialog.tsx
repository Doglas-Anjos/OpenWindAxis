/**
 * Status do anexo (manual dlg-file-append-status.htm): one row per appended
 * file, with what became of it: appended (its steps, the columns it added
 * or linked by properties to a column of another name) or refused, and why.
 */
import type { AppendReport } from '../workbook/useWorkbook';
import { formatCount } from '../format';
import { Modal } from './Modal';

/** pt-BR text for the reasons a file is refused; other codes show the core's message. */
const REFUSAL_TEXT: Record<string, string> = {
  append_step_mismatch: 'passo de tempo diferente do dataset',
  append_out_of_phase: 'carimbos fora de fase com os do dataset',
  timestamps_span_too_many: 'longe demais do dataset no tempo',
  too_few_timestamps: 'menos de duas linhas com carimbo de tempo',
};

export function AppendStatusDialog({ report, onClose }: { report: AppendReport; onClose: () => void }) {
  const refused = report.statuses.filter((status) => status.error).length;
  return (
    <Modal
      title={`Status do anexo — ${report.dataset}`}
      onClose={onClose}
      wide
      buttons={
        <button className="btn primary" onClick={onClose}>
          OK
        </button>
      }
    >
      <p className="hint">
        {formatCount(report.statuses.length - refused)} arquivo(s) anexado(s)
        {refused ? `, ${formatCount(refused)} recusado(s)` : ''}. Onde o dataset já tinha dados, eles ficam; os
        anexados só preenchem as lacunas.
      </p>
      <div className="tablewrap appendstatus">
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th>Arquivo</th>
              <th>Situação</th>
              <th className="r">Passos</th>
              <th>Colunas</th>
            </tr>
          </thead>
          <tbody>
            {report.statuses.map((status, position) => (
              <tr key={position} className={status.error ? 'refused' : undefined}>
                <td>{position + 1}</td>
                <td className="id">{status.name}</td>
                <td title={status.error?.message}>
                  {status.error ? `Recusado: ${REFUSAL_TEXT[status.error.code] ?? status.error.message}` : 'Sucesso'}
                </td>
                <td className="r">{status.error ? '' : formatCount(status.steps)}</td>
                <td>
                  {status.newColumns.length > 0 && `${status.newColumns.length} nova(s): ${status.newColumns.join(', ')}`}
                  {status.linked.map(([appended, master]) => (
                    <div key={appended}>
                      {appended} → {master}
                    </div>
                  ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Modal>
  );
}
