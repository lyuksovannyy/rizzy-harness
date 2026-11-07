// The cards for `ui:` blocks, in their own lazy chunk: a chat without
// blocks never loads them. Each spec was validated and cleaned in Rust and
// every string renders as React text (no Markdown, no links). The chart is
// a read-only table of its data until the drawn chart replaces it.
import type { ChartSpec, UiBlock as Block } from "../types";
import type { ProgressSlot } from "./blocks";
import Choices from "./Choices";
import Form from "./Form";
import { PREVIEW, type InputProps } from "./Input";
import Progress from "./Progress";

/** `slot` places a progress block in its turn (full or snapshot, ended);
 * `input` is where a question or form stands and where its answer goes. */
export default function UiBlock({ block, slot, input }: { block: Block; slot?: ProgressSlot; input?: InputProps }) {
  const answer = input ?? { sessionId: null, messageId: null, blockId: block.id, info: PREVIEW };
  switch (block.type) {
    case "choices":
      return block.spec ? <Choices spec={block.spec} input={answer} /> : null;
    case "form":
      return block.spec ? <Form spec={block.spec} input={answer} /> : null;
    case "progress":
      return block.spec ? <Progress spec={block.spec} slot={slot} /> : null;
    case "chart":
      return block.spec ? <Chart spec={block.spec} /> : null;
  }
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
