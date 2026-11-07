// The cards for `ui:` blocks, in their own lazy chunk: a chat without
// blocks never loads them. Each spec was validated and cleaned in Rust and
// every string renders as React text (no Markdown, no links). Questions,
// forms and charts are read-only views of the spec for now; answering and
// the drawn chart replace them type by type.
import type { ChartSpec, ChoicesSpec, Field, FormSpec, UiBlock as Block } from "../types";
import { rt } from "../md/strings";
import type { ProgressSlot } from "./blocks";
import Progress from "./Progress";

/** `slot` places a progress block in its turn (full or snapshot, ended). */
export default function UiBlock({ block, slot }: { block: Block; slot?: ProgressSlot }) {
  switch (block.type) {
    case "choices":
      return block.spec ? <Choices spec={block.spec} /> : null;
    case "form":
      return block.spec ? <Form spec={block.spec} /> : null;
    case "progress":
      return block.spec ? <Progress spec={block.spec} slot={slot} /> : null;
    case "chart":
      return block.spec ? <Chart spec={block.spec} /> : null;
  }
}

function Choices({ spec }: { spec: ChoicesSpec }) {
  return (
    <div className="ui-choices">
      <p className="ui-title">{spec.question}</p>
      <ul className="ui-options">
        {spec.options.map((o) => (
          <li key={o.id}>
            <span className="ui-option-label">{o.label}</span>
            {o.description && <span className="muted"> · {o.description}</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Form({ spec }: { spec: FormSpec }) {
  return (
    <div className="ui-form">
      <p className="ui-title">{spec.title}</p>
      {spec.description && <p className="ui-description">{spec.description}</p>}
      <ul className="ui-fields">
        {spec.fields.map((f) => (
          <li key={f.id}>{f.refused ? <Refused field={f} /> : <FieldRow field={f} />}</li>
        ))}
      </ul>
    </div>
  );
}

function FieldRow({ field }: { field: Field }) {
  return (
    <>
      <span className="ui-field-label">{field.label}</span>
      {field.required && <span className="muted"> ({rt("ui.required")})</span>}
      {field.help && <div className="muted small">{field.help}</div>}
    </>
  );
}

/** A field that asks for a secret: never an input, only the warning. */
function Refused({ field }: { field: Field }) {
  return (
    <div className="ui-secret" role="note">
      <span className="ui-field-label">{field.label}</span>: {rt("ui.secret")}
    </div>
  );
}

function Chart({ spec }: { spec: ChartSpec }) {
  const unit = spec.y?.unit ? ` (${spec.y.unit})` : "";
  return (
    <div className="ui-chart-data">
      {spec.title && <p className="ui-title">{spec.title}</p>}
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>{spec.x.label ?? ""}</th>
              {spec.series.map((s, i) => (
                <th key={i}>
                  {s.name}
                  {unit}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {spec.x.values.map((x, row) => (
              <tr key={row}>
                <td>{x}</td>
                {spec.series.map((s, i) => (
                  <td key={i}>{s.values[row] ?? ""}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
