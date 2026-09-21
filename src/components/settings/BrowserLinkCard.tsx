import { AnimatePresence, motion } from 'motion/react';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  CheckCircle2,
  CircleAlert,
  Copy,
  ExternalLink,
  Link2Off,
  LogIn,
  Puzzle,
  TriangleAlert,
  Wrench,
} from 'lucide-react';
import { useEffect, useState, type ReactNode } from 'react';

import { Badge, type BadgeTone } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { SettingGroup, SettingRow, ToggleRow } from '@/components/ui/SettingRow';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { COLLAPSE, T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { useToastStore } from '@/stores/useToastStore';
import type { BridgeStatus, Settings } from '@/types';

/**
 * How often the section re-reads the link while it is on screen.
 *
 * The browser pushes to a helper process, not to the app, so a session can
 * appear while this page is open and nothing in the app would know. The event
 * covers what the app itself changes; the poll covers what the browser does,
 * and it costs one small file read.
 */
const POLL_MS = 4000;

/**
 * The link's state, refreshed while the component is mounted.
 *
 * `pollMs` of 0 reads once and then only follows the event, which is all the
 * Settings page itself needs to decide whether the section exists at all.
 */
export function useBridgeStatus(pollMs = 0): BridgeStatus | null {
  const [status, setStatus] = useState<BridgeStatus | null>(null);

  useEffect(() => {
    // There is no bridge on a phone, and no command behind these names either.
    if (IS_MOBILE) return;

    let active = true;
    const read = () => {
      ipc
        .bridgeStatus()
        .then((next) => {
          if (active) setStatus(next);
        })
        .catch(() => {
          // A failed read leaves the last known state on screen rather than
          // flashing "not connected" at someone whose link is fine.
        });
    };

    read();
    const timer = pollMs > 0 ? window.setInterval(read, pollMs) : null;
    const unlisten = ipc.onBridgeChanged(read);

    return () => {
      active = false;
      if (timer !== null) window.clearInterval(timer);
      void unlisten.then((off) => off());
    };
  }, [pollMs]);

  return status;
}

/**
 * Which of the link's states the user is looking at.
 *
 * Ordered by what has to be fixed first: a browser that cannot start the helper
 * makes every other question moot, and a stale session is reported as its own
 * state rather than as a connection, because a card that says "connected" while
 * the popup says otherwise is worse than either being wrong alone.
 */
type LinkPhase = 'broken' | 'waiting' | 'signedOut' | 'quiet' | 'connected';

function phaseOf(status: BridgeStatus): LinkPhase {
  if (!status.registered) return 'broken';
  if (!status.connected) return 'waiting';
  if (status.session === 'stale') return 'quiet';
  if (status.session === 'none') return 'signedOut';
  return 'connected';
}

interface Presentation {
  icon: ReactNode;
  iconClass: string;
  tone: BadgeTone;
  badge: TranslationKey;
  title: TranslationKey;
  body: TranslationKey;
}

const PHASES: Record<LinkPhase, Presentation> = {
  broken: {
    icon: <Wrench size={17} />,
    iconClass: 'bg-error-soft text-error',
    tone: 'error',
    badge: 'settings.linkBadgeBroken',
    title: 'settings.linkBroken',
    body: 'settings.linkBrokenHint',
  },
  waiting: {
    icon: <Puzzle size={17} />,
    iconClass: 'bg-accent-soft text-accent',
    tone: 'neutral',
    badge: 'settings.linkBadgeWaiting',
    title: 'settings.linkWaiting',
    body: 'settings.linkWaitingHint',
  },
  signedOut: {
    icon: <LogIn size={17} />,
    iconClass: 'bg-warning-soft text-warning',
    tone: 'warning',
    badge: 'settings.linkBadgeSignedOut',
    title: 'settings.linkSignedOut',
    body: 'settings.linkSignedOutHint',
  },
  quiet: {
    icon: <CircleAlert size={17} />,
    iconClass: 'bg-warning-soft text-warning',
    tone: 'warning',
    badge: 'settings.linkBadgeQuiet',
    title: 'settings.linkQuiet',
    body: 'settings.linkQuietHint',
  },
  connected: {
    icon: <CheckCircle2 size={17} />,
    iconClass: 'bg-success-soft text-success',
    tone: 'success',
    badge: 'settings.linkBadgeConnected',
    title: 'settings.linkConnected',
    body: 'settings.linkConnectedHint',
  },
};

/**
 * How long ago the browser last handed the session over, in the reader's own
 * language. Relative rather than a timestamp: the question this line answers is
 * whether the connection is alive, and "three days ago" answers it where a date
 * and a time leave the arithmetic to the user.
 */
function relativeTime(epochSeconds: number, locale: string): string {
  const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ['second', 60],
    ['minute', 60],
    ['hour', 24],
    ['day', 7],
    ['week', 4.35],
    ['month', 12],
  ];

  let value = epochSeconds - Date.now() / 1000;
  for (const [unit, span] of steps) {
    if (Math.abs(value) < span) return format.format(Math.round(value), unit);
    value /= span;
  }
  return format.format(Math.round(value), 'year');
}

interface BrowserLinkCardProps {
  settings: Settings;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export function BrowserLinkCard({ settings, update }: BrowserLinkCardProps) {
  const { t, language } = useTranslation();
  const pushToast = useToastStore((state) => state.push);
  const status = useBridgeStatus(POLL_MS);
  const [working, setWorking] = useState(false);

  // Repair and Disconnect both make the backend emit `bridge://changed`, so the
  // card is refreshed by the same path a push from the browser takes; there is
  // no second copy of the state here to keep in step.
  const repair = async () => {
    setWorking(true);
    try {
      const next = await ipc.bridgeRepair();
      pushToast(
        next.registered
          ? { tone: 'success', title: t('settings.linkRepaired') }
          : { tone: 'error', title: t('settings.linkRepairFailed') },
      );
    } catch {
      pushToast({ tone: 'error', title: t('settings.linkRepairFailed') });
    } finally {
      setWorking(false);
    }
  };

  const disconnect = async () => {
    setWorking(true);
    try {
      await ipc.bridgeDisconnect();
      pushToast({ tone: 'success', title: t('settings.linkDisconnected') });
    } catch {
      pushToast({ tone: 'error', title: t('settings.linkDisconnectFailed') });
    } finally {
      setWorking(false);
    }
  };

  const copyDiagnostics = async () => {
    try {
      await navigator.clipboard.writeText(await ipc.bridgeDiagnostics());
      pushToast({ tone: 'success', title: t('settings.linkDiagnosticsCopied') });
    } catch {
      pushToast({ tone: 'error', title: t('settings.linkDiagnosticsFailed') });
    }
  };

  const phase = status ? phaseOf(status) : null;
  const look = phase ? PHASES[phase] : null;
  const browser = status?.browser ?? t('settings.linkBrowserFallback');

  // "Chrome is connected" is not an answer when two Chrome windows are open, so
  // a connected link is headed by the profile as well whenever the extension
  // could see one.
  let title = look ? t(look.title, { browser }) : '';
  if (phase === 'connected' && status?.profileLabel) {
    title = t('settings.linkConnectedProfile', { browser, profile: status.profileLabel });
  }

  return (
    <SettingGroup title={t('settings.connection')}>
      <ToggleRow
        title={t('settings.browserLink')}
        description={t('settings.browserLinkHint')}
        checked={settings.browserLinkEnabled}
        onChange={(value) => void update({ browserLinkEnabled: value })}
      />

      <AnimatePresence initial={false}>
        {settings.browserLinkEnabled && (
          <motion.div
            variants={COLLAPSE}
            initial="initial"
            animate="animate"
            exit="exit"
            className="overflow-hidden"
          >
            {/* Nothing at all until the first read answers: an empty frame
                that fills in a moment later is a flicker, not information. */}
            {status && look && (
              <div className="px-4 py-4">
                <motion.div
                  key={phase}
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={T.component}
                  className="flex items-start gap-3"
                >
                  <span
                    className={cn(
                      'mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg',
                      look.iconClass,
                    )}
                  >
                    {look.icon}
                  </span>

                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-[13.5px] font-medium text-fg">{title}</span>
                      <Badge tone={look.tone}>{t(look.badge)}</Badge>
                      {status.extensionVersion && (
                        <Badge tone="outline">{status.extensionVersion}</Badge>
                      )}
                    </div>

                    <p className="mt-1 text-[12.5px] leading-relaxed text-fg-muted">
                      {t(look.body, { browser })}
                    </p>

                    {/* Two Chrome windows look identical from here, so a link
                        the user cannot place is a link they cannot trust. */}
                    {phase === 'connected' && !status.profileLabel && (
                      <p className="mt-1.5 text-[12.5px] leading-relaxed text-fg-faint">
                        {t('settings.linkNoProfileName')}
                      </p>
                    )}

                    {/* Both of these describe a browser that is bound; beside
                        "no browser connected" they would describe a ghost. */}
                    {status.connected && status.accountHint && (
                      <p className="mt-1.5 text-[12.5px] text-fg-muted">
                        {t('settings.linkAccount', { account: status.accountHint })}
                      </p>
                    )}

                    {status.connected && status.lastPushAt != null && (
                      <p className="mt-1 text-[12.5px] text-fg-faint">
                        {t('settings.linkRefreshed', {
                          when: relativeTime(status.lastPushAt, language),
                        })}
                      </p>
                    )}

                    {phase === 'waiting' && !status.storeListed && (
                      <p className="mt-2 text-[12.5px] leading-relaxed text-fg-faint">
                        {t('settings.linkStorePending')}
                      </p>
                    )}

                    <div className="mt-3 flex flex-wrap items-center gap-2">
                      {phase === 'broken' && (
                        <Button
                          size="sm"
                          variant="primary"
                          loading={working}
                          icon={<Wrench size={13} />}
                          onClick={() => void repair()}
                        >
                          {t('settings.linkRepair')}
                        </Button>
                      )}
                      {phase === 'waiting' && status.storeListed && (
                        <Button
                          size="sm"
                          variant="primary"
                          icon={<ExternalLink size={13} />}
                          onClick={() =>
                            void openUrl(
                              `https://chromewebstore.google.com/detail/${status.extensionId}`,
                            )
                          }
                        >
                          {t('settings.linkGetExtension')}
                        </Button>
                      )}
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<Copy size={13} />}
                        onClick={() => void copyDiagnostics()}
                      >
                        {t('settings.linkCopyDiagnostics')}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={working}
                        icon={<Link2Off size={13} />}
                        onClick={() => void disconnect()}
                      >
                        {t('settings.linkDisconnect')}
                      </Button>
                    </div>

                    <p className="mt-2 text-[11.5px] leading-relaxed text-fg-faint">
                      {t('settings.linkDiagnosticsHint')}
                    </p>
                  </div>
                </motion.div>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* The failure this design is most prone to, and the one a user reads as
          "the connection is broken" unless the app names it first. */}
      {settings.browserLinkEnabled && (
        <SettingRow
          title={
            <span className="flex items-center gap-1.5">
              <TriangleAlert size={14} className="shrink-0 text-warning" />
              {t('settings.linkSecondAccount')}
            </span>
          }
          description={t('settings.linkSecondAccountHint')}
        />
      )}
    </SettingGroup>
  );
}
