import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useT } from "@/i18n";
import { api } from "@/lib/api";

interface DemoAccount {
  role: "teacher" | "student";
  label: string;
  account: string;
  password: string;
  language: string;
}

/**
 * 示範模式的帳號面板（發表用）：後端開了 `DEMO_MODE` 才有清單，沒開就是 404，面板不出現。
 * 只把帳號與公開的示範密碼填進表單，不會自己登入；登入仍走一般的密碼登入。
 */
export function DemoAccounts({
  onPick,
}: {
  onPick: (account: string, password: string) => void;
}) {
  const t = useT();
  const [accounts, setAccounts] = useState<DemoAccount[]>([]);

  useEffect(() => {
    const controller = new AbortController();
    api
      .get<{ accounts: DemoAccount[] }>("/api/demo/accounts", controller.signal)
      .then((res) => setAccounts(res.accounts))
      .catch(() => {});
    return () => controller.abort();
  }, []);

  if (accounts.length === 0) return null;
  const groups = [
    {
      title: t("login.demo.teacher"),
      items: accounts.filter((a) => a.role === "teacher"),
    },
    {
      title: t("login.demo.student"),
      items: accounts.filter((a) => a.role === "student"),
    },
  ];
  return (
    <section
      aria-labelledby="demo-accounts-title"
      className="border-border flex flex-col gap-3 rounded-md border border-dashed p-3"
    >
      <div>
        <h2 id="demo-accounts-title" className="text-sm font-medium">
          {t("login.demo.title")}
        </h2>
        <p className="text-muted-foreground text-xs">{t("login.demo.hint")}</p>
      </div>
      {groups.map((g) => (
        <div key={g.title} className="flex flex-col gap-1.5">
          <p className="text-muted-foreground text-xs">{g.title}</p>
          <div className="flex flex-wrap gap-1.5">
            {g.items.map((a) => (
              <Button
                key={a.account}
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => onPick(a.account, a.password)}
              >
                {a.label}
                {a.language !== "zh-TW" ? ` (${a.language})` : ""}
              </Button>
            ))}
          </div>
        </div>
      ))}
    </section>
  );
}
