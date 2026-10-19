/**
 * Definir flags (ESPEC-INTERFACE §5.2): the dataset's flags, each with its
 * name, colour, notes and whether analyses keep its data by default. Add,
 * edit and delete here; OK applies it all at once. A deleted flag takes its
 * stretches, their evidence and the rules that set it, and the undo history
 * restarts, since its steps speak of the old flags.
 */
import { useState } from "react";
import type { WindApi } from "../api/client";
import type { FlagDto } from "../api/generated/FlagDto";
import { errorMessage, formatCount } from "../format";
import type { Entry } from "../workbook/model";
import type { Workbook } from "../workbook/useWorkbook";
import { Modal } from "./Modal";

/** Colours offered to new flags, in turn. */
const NEW_FLAG_COLORS = ["#e08a1e", "#3d9b4f", "#c2408f", "#6b7a8f", "#b5a000"];
const NEW_FLAG_NAME = "Nova flag";

/** A flag being edited, with the index it had (none: added here). */
interface Draft {
  flag: FlagDto;
  was?: number;
}

interface Props {
  api: WindApi;
  entry: Entry;
  workbook: Workbook;
  onClose: () => void;
}

export function DefineFlagsDialog({ api, entry, workbook, onClose }: Props) {
  const { dataset } = entry.result;
  const [drafts, setDrafts] = useState<Draft[]>(() =>
    dataset.flags.map((flag, index) => ({ flag, was: index })),
  );
  const [chosen, setChosen] = useState(0);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const stretchesOf = (flag: number) =>
    dataset.segments.filter((segment) => segment.flag === flag).length;
  const rulesOf = (flag: number) =>
    entry.rules.filter((rule) => rule.flag === flag).length;
  const deleted = dataset.flags
    .map((_, index) => index)
    .filter((index) => !drafts.some((draft) => draft.was === index));
  const current = drafts[chosen];
  const change = (patch: Partial<FlagDto>) =>
    setDrafts(
      drafts.map((draft, index) =>
        index === chosen
          ? { ...draft, flag: { ...draft.flag, ...patch } }
          : draft,
      ),
    );

  const add = () => {
    const taken = new Set(
      drafts.map((draft) => draft.flag.name.trim().toLowerCase()),
    );
    let name = NEW_FLAG_NAME;
    for (let suffix = 2; taken.has(name.toLowerCase()); suffix++)
      name = `${NEW_FLAG_NAME} ${suffix}`;
    const color = NEW_FLAG_COLORS[drafts.length % NEW_FLAG_COLORS.length];
    setDrafts([
      ...drafts,
      { flag: { name, color, include: false, notes: "" } },
    ]);
    setChosen(drafts.length);
  };
  const remove = () => {
    setDrafts(drafts.filter((_, index) => index !== chosen));
    setChosen(Math.max(0, chosen - 1));
  };
  const apply = async () => {
    setSaving(true);
    try {
      const state = await api.defineFlags({
        dataset: dataset.id,
        flags: drafts,
      });
      // The rules follow their flag to its new place; a deleted flag's rules go with it.
      const newIndex = new Map(
        drafts.flatMap((draft, index) =>
          draft.was === undefined ? [] : [[draft.was, index] as const],
        ),
      );
      const rules = entry.rules.flatMap((rule) =>
        newIndex.has(rule.flag)
          ? [{ ...rule, flag: newIndex.get(rule.flag)! }]
          : [],
      );
      workbook.updateEntry(dataset.id, () => ({ rules, dirty: true }));
      await workbook.showEdit(entry, Promise.resolve(state));
      onClose();
    } catch (thrown) {
      setError(errorMessage(thrown));
      setSaving(false);
    }
  };

  return (
    <Modal
      title={`Definir flags: ${dataset.name}`}
      onClose={onClose}
      buttons={
        <>
          <button className="btn primary" disabled={saving} onClick={apply}>
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancelar
          </button>
        </>
      }
    >
      <div className="define-flags">
        <div>
          <div className="tablewrap">
            <table className="grid">
              <thead>
                <tr>
                  <th>Flag</th>
                  <th>Nos cálculos</th>
                  <th className="r">Trechos</th>
                </tr>
              </thead>
              <tbody>
                {drafts.map((draft, index) => (
                  <tr
                    key={index}
                    className={index === chosen ? "sel" : undefined}
                    onClick={() => setChosen(index)}
                  >
                    <td>
                      <span
                        className="swatch"
                        style={{ background: draft.flag.color }}
                      />{" "}
                      {draft.flag.name}
                    </td>
                    <td>{draft.flag.include ? "incluída" : "excluída"}</td>
                    <td className="r">
                      {draft.was === undefined
                        ? "nova"
                        : formatCount(stretchesOf(draft.was))}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="actions">
            <button className="btn" onClick={add}>
              Adicionar
            </button>
            <button className="btn" disabled={!current} onClick={remove}>
              Excluir
            </button>
          </div>
        </div>
        {current && (
          <div className="properties">
            <fieldset>
              <legend>Propriedades</legend>
              <label>
                Nome
                <input
                  value={current.flag.name}
                  onChange={(event) => change({ name: event.target.value })}
                />
              </label>
              <label>
                Cor
                <input
                  type="color"
                  value={current.flag.color}
                  onChange={(event) => change({ color: event.target.value })}
                />
              </label>
              <label>
                Notas
                <textarea
                  rows={3}
                  value={current.flag.notes}
                  onChange={(event) => change({ notes: event.target.value })}
                />
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={current.flag.include}
                  onChange={(event) =>
                    change({ include: event.target.checked })
                  }
                />
                Incluir nos cálculos por padrão
              </label>
            </fieldset>
          </div>
        )}
      </div>
      {deleted.length > 0 && (
        <div className="note warn">
          Ao confirmar, sai{deleted.length > 1 ? "em" : ""}{" "}
          {deleted
            .map(
              (index) =>
                `${dataset.flags[index].name} (${formatCount(stretchesOf(index))} trechos, ${formatCount(rulesOf(index))} regras)`,
            )
            .join(", ")}
          , com seus trechos e regras, e o histórico de desfazer do dataset
          recomeça.
        </div>
      )}
      {error && <div className="note err">{error}</div>}
      <p className="hint">
        Dado sob uma flag excluída dos cálculos fica de fora de toda análise, a
        menos que o "Filtrar por" da janela diga outra coisa.
      </p>
    </Modal>
  );
}
