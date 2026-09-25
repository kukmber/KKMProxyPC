import { useState } from "react";
import {
  Button,
  Menu,
  MenuItem,
  MenuList,
  MenuPopover,
  MenuTrigger,
  ProgressBar,
  Radio,
  Spinner,
  Tooltip,
} from "@fluentui/react-components";
import {
  AddRegular,
  ArrowClockwiseRegular,
  CopyRegular,
  DeleteRegular,
  MoreHorizontalRegular,
  RenameRegular,
} from "@fluentui/react-icons";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, formatAgo, formatBytes, Profile } from "../api";
import { useNotify } from "../toast";
import { AddProfileDialog } from "./AddProfileDialog";
import { RenameDialog } from "./RenameDialog";

export function ProfilesSection(props: { profiles: Profile[]; activeId?: string; onChanged: () => void }) {
  const notify = useNotify();
  const [adding, setAdding] = useState(false);
  const [refreshing, setRefreshing] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<Profile | null>(null);

  const refresh = async (p: Profile) => {
    setRefreshing(p.id);
    try {
      await api.refreshProfile(p.id);
      notify.ok("Подписка обновлена", p.name);
      props.onChanged();
    } catch (e) {
      notify.error("Не удалось обновить подписку", e);
    } finally {
      setRefreshing(null);
    }
  };

  const activate = async (p: Profile) => {
    if (p.id === props.activeId) return;
    try {
      await api.setActiveProfile(p.id);
      props.onChanged();
    } catch (e) {
      notify.error("Не удалось переключить подписку", e);
    }
  };

  const remove = async (p: Profile) => {
    try {
      await api.deleteProfile(p.id);
      props.onChanged();
    } catch (e) {
      notify.error("Не удалось удалить", e);
    }
  };

  return (
    <>
      <div className="section-title">
        Подписки
        <span className="spacer" />
        <Button appearance="subtle" icon={<AddRegular />} onClick={() => setAdding(true)}>
          Добавить
        </Button>
      </div>

      <div className="card">
        {props.profiles.length === 0 && (
          <div className="row" style={{ minHeight: 88 }}>
            <div className="text">
              <span className="title">Подписок пока нет</span>
              <span className="desc">Вставьте ссылку на подписку, отдельный сервер (vless://, hysteria2://…) или выберите файл конфига.</span>
            </div>
            <Button appearance="primary" onClick={() => setAdding(true)}>
              Добавить подписку
            </Button>
          </div>
        )}
        {props.profiles.map((p) => {
          const u = p.userinfo;
          const used = u ? u.upload + u.download : 0;
          const details = [
            `${p.proxyCount || "?"} ${plural(p.proxyCount, "сервер", "сервера", "серверов")}`,
            p.url ? `обновлена ${formatAgo(p.updatedAt)}` : "из файла",
            u && u.total > 0 ? `${formatBytes(used)} из ${formatBytes(u.total)}` : u && used > 0 ? `использовано ${formatBytes(used)}` : null,
            u?.expire ? `до ${new Date(u.expire * 1000).toLocaleDateString("ru-RU")}` : null,
          ].filter(Boolean);
          return (
            <div key={p.id} className="row clickable" onClick={() => activate(p)} style={{ cursor: "pointer" }}>
              <Radio checked={p.id === props.activeId} onChange={() => activate(p)} aria-label={p.name} />
              <div className="text">
                <span className="title ellipsis">{p.name}</span>
                <span className="desc">{details.join(" · ")}</span>
                {u && u.total > 0 && (
                  <ProgressBar
                    value={Math.min(used / u.total, 1)}
                    thickness="medium"
                    color={used / u.total > 0.9 ? "warning" : "brand"}
                    style={{ maxWidth: 320, marginTop: 4 }}
                  />
                )}
              </div>
              <div onClick={(e) => e.stopPropagation()} style={{ display: "flex", gap: 4 }}>
                {p.url && (
                  <Tooltip content="Обновить" relationship="label">
                    <Button
                      appearance="subtle"
                      icon={refreshing === p.id ? <Spinner size="tiny" /> : <ArrowClockwiseRegular />}
                      disabled={refreshing !== null}
                      onClick={() => refresh(p)}
                    />
                  </Tooltip>
                )}
                <Menu>
                  <MenuTrigger disableButtonEnhancement>
                    <Button appearance="subtle" icon={<MoreHorizontalRegular />} aria-label="Ещё" />
                  </MenuTrigger>
                  <MenuPopover>
                    <MenuList>
                      <MenuItem icon={<RenameRegular />} onClick={() => setRenaming(p)}>
                        Переименовать
                      </MenuItem>
                      {p.url && (
                        <MenuItem
                          icon={<CopyRegular />}
                          onClick={() => writeText(p.url!).then(() => notify.ok("Ссылка скопирована"))}
                        >
                          Копировать ссылку
                        </MenuItem>
                      )}
                      <MenuItem icon={<DeleteRegular />} onClick={() => remove(p)}>
                        Удалить
                      </MenuItem>
                    </MenuList>
                  </MenuPopover>
                </Menu>
              </div>
            </div>
          );
        })}
      </div>

      <AddProfileDialog
        open={adding}
        onClose={() => setAdding(false)}
        onAdded={(p) => {
          setAdding(false);
          notify.ok("Подписка добавлена", `${p.name} — ${p.proxyCount} ${plural(p.proxyCount, "сервер", "сервера", "серверов")}`);
          props.onChanged();
        }}
      />
      <RenameDialog
        profile={renaming}
        onClose={() => setRenaming(null)}
        onDone={() => {
          setRenaming(null);
          props.onChanged();
        }}
      />
    </>
  );
}

export function plural(n: number, one: string, few: string, many: string) {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few;
  return many;
}
