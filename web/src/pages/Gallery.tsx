import { useState } from "react";
import { TableRenderer } from "../components/TableRenderer";
import gallery from "../generated/gallery.json";
import type { TableView } from "../protocol";

interface Entry {
  id: string;
  name: string;
  family: string;
  /** Mid-hand and end-of-hand views from seat 0, recorded by `cargo test -p server`. */
  views: TableView[];
}

/**
 * `#/dev/views` (dev builds only): every game's `TableView`, drawn by the one generic renderer —
 * the visual check for each game family (web.md §8, W6).
 */
export function Gallery() {
  const entries = gallery as unknown as Entry[];
  const [filter, setFilter] = useState("");
  const shown = entries.filter(
    (e) => !filter || e.name.toLowerCase().includes(filter.toLowerCase()) || e.family.toLowerCase().includes(filter.toLowerCase()),
  );
  return (
    <div className="gallery">
      <h2>View gallery ({entries.length} games)</h2>
      <input type="search" placeholder="Filter" value={filter} onChange={(e) => setFilter(e.target.value)} />
      {shown.map((e) => (
        <section key={e.id} id={e.id}>
          <h3>
            {e.name} <span className="muted">{e.family}</span>
          </h3>
          <div className="gallery-row">
            {e.views.map((v, i) => (
              <TableRenderer key={i} view={v} labels={(s) => ({ name: `Player ${s + 1}` })} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
