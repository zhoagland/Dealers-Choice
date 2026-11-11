import { useEffect, useState } from "react";
import { api } from "../api";
import { money } from "../cards";
import type { AdminAccount } from "../protocol";

/** Admins deposit play money or reset a bankroll (D-WEB-3). */
export function Admin() {
  const [rows, setRows] = useState<AdminAccount[]>([]);
  const [amount, setAmount] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.adminAccounts().then(setRows, (e: Error) => setError(e.message));
  }, []);
  const apply = (id: number, op: "deposit" | "reset") => {
    const dollars = Number(amount[id] ?? "");
    if (!Number.isFinite(dollars) || dollars < 0) return setError("enter an amount in dollars");
    api
      .adminBankroll({ account: id, op, cents: Math.round(dollars * 100) })
      .then((a) => setRows((r) => r.map((x) => (x.id === a.id ? a : x))), (e: Error) => setError(e.message));
  };
  return (
    <div className="admin">
      <h2>Bankrolls</h2>
      {error && <p className="error">{error}</p>}
      <table>
        <thead>
          <tr>
            <th>Player</th>
            <th>Bankroll</th>
            <th>Amount ($)</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((a) => (
            <tr key={a.id}>
              <td>
                {a.name}
                {a.is_admin ? " (admin)" : ""}
              </td>
              <td>{money(a.bankroll)}</td>
              <td>
                <input
                  inputMode="decimal"
                  value={amount[a.id] ?? ""}
                  onChange={(e) => setAmount({ ...amount, [a.id]: e.target.value })}
                  aria-label={`amount for ${a.name}`}
                />
              </td>
              <td>
                <button className="btn" onClick={() => apply(a.id, "deposit")}>
                  Deposit
                </button>{" "}
                <button className="btn" onClick={() => apply(a.id, "reset")}>
                  Reset to
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
