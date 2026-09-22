import { AnimatePresence, motion } from 'motion/react';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  Check,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Copy,
  ExternalLink,
  TriangleAlert,
  type LucideIcon,
} from 'lucide-react';
import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/Button';
import { InlineNotice } from '@/components/ui/InlineNotice';
import { SettingGroup, SettingRow, ToggleRow } from '@/components/ui/SettingRow';
import { useMomentary } from '@/hooks/useMomentary';
import { useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { COLLAPSE } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
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
  icon: LucideIcon;
  iconClass: string;
  title: TranslationKey;
  body: TranslationKey;
}

const PHASES: Record<LinkPhase, Presentation> = {
  broken: {
    icon: CircleAlert,
    iconClass: 'text-error',
    title: 'settings.linkBroken',
    body: 'settings.linkBrokenHint',
  },
  waiting: {
    icon: CircleDashed,
    iconClass: 'text-fg-muted',
    title: 'settings.linkWaiting',
    body: 'settings.linkWaitingHint',
  },
  signedOut: {
    icon: CircleAlert,
    iconClass: 'text-warning',
    title: 'settings.linkSignedOut',
    body: 'settings.linkSignedOutHint',
  },
  quiet: {
    icon: CircleAlert,
    iconClass: 'text-warning',
    title: 'settings.linkQuiet',
    body: 'settings.linkQuietHint',
  },
  connected: {
    icon: CircleCheck,
    iconClass: 'text-success',
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
  const status = useBridgeStatus(POLL_MS);
  const [working, setWorking] = useState(false);
  const [problem, setProblem] = useState<TranslationKey | null>(null);
  const [copied, markCopied] = useMomentary();

  // Repair and Disconnect both make the backend emit `bridge://changed`, so the
  // card is refreshed by the same path a push from the browser takes; there is
  // no second copy of the state here to keep in step. That refresh is also the
  // confirmation: the state above the buttons changes. Only a failure has to be
  // put into words, because it changes nothing.
  const repair = async () => {
    setProblem(null);
    setWorking(true);
    try {
      const next = await ipc.bridgeRepair();
      if (!next.registered) setProblem('settings.linkRepairFailed');
    } catch {
      setProblem('settings.linkRepairFailed');
    } finally {
      setWorking(false);
    }
  };

  const disconnect = async () => {
    setProblem(null);
    setWorking(true);
    try {
      await ipc.bridgeDisconnect();
    } catch {
      setProblem('settings.linkDisconnectFailed');
    } finally {
      setWorking(false);
    }
  };

  const copyDiagnostics = async () => {
    setProblem(null);
    try {
      await navigator.clipboard.writeText(await ipc.bridgeDiagnostics());
      // Nothing on screen changes when text reaches the clipboard, so the
      // button itself says that it did.
      markCopied();
    } catch {
      setProblem('settings.linkDiagnosticsFailed');
    }
  };

  const phase = status ? phaseOf(status) : null;
  const look = phase ? PHASES[phase] : null;
  const StateIcon = look?.icon;
  // Whether a filled button -- Repair, or Get the extension -- heads the actions.
  const leadAction = phase === 'broken' || (phase === 'waiting' && status?.storeListed === true);
  const browser = status?.browser ?? t('settings.linkBrowserFallback');
  // The sizes a `SettingRow` sets its two lines in, so the rows of the group agree.
  const titleSize = IS_MOBILE ? 'text-[15px]' : 'text-[13.5px]';
  const bodySize = IS_MOBILE ? 'text-[13px]' : 'text-[12.5px]';

  // "Chrome is connected" is not an answer when two Chrome windows are open, so
  // a connected link is headed by the profile as well whenever the extension
  // could see one.
  let title = look ? t(look.title, { browser }) : '';
  if (phase === 'connected' && status?.profileLabel) {
    title = t('settings.linkConnectedProfile', { browser, profile: status.profileLabel });
  }

  return (
    <SettingGroup>
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
            {status && look && StateIcon && (
              <div className={cn('px-4', IS_MOBILE ? 'py-4' : 'py-3.5')}>
                <div className="flex items-start gap-2">
                  <StateIcon
                    size={16}
                    aria-hidden="true"
                    className={cn('mt-[2px] shrink-0', look.iconClass)}
                  />
                  <span className={cn('min-w-0 text-fg', titleSize)}>{title}</span>
                </div>

                <p className={cn('mt-1 leading-relaxed text-fg-muted', bodySize)}>
                  {t(look.body, { browser })}
                </p>

                {/* Two Chrome windows look identical from here, so a link
                    the user cannot place is a link they cannot trust. */}
                {phase === 'connected' && !status.profileLabel && (
                  <p className={cn('mt-1.5 leading-relaxed text-fg-muted', bodySize)}>
                    {t('settings.linkNoProfileName')}
                  </p>
                )}

                {/* All of these describe a browser that is bound; beside
                    "no browser connected" they would describe a ghost. */}
                {status.connected &&
                  (status.accountHint || status.lastPushAt != null || status.extensionVersion) && (
                    <div
                      className={cn('mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5 text-fg-muted', bodySize)}
                    >
                      {status.accountHint && (
                        <span>{t('settings.linkAccount', { account: status.accountHint })}</span>
                      )}
                      {status.lastPushAt != null && (
                        <span>
                          {t('settings.linkRefreshed', {
                            when: relativeTime(status.lastPushAt, language),
                          })}
                        </span>
                      )}
                      {status.extensionVersion && (
                        <span className="tabular">
                          {t('settings.linkExtensionVersion', {
                            version: status.extensionVersion,
                          })}
                        </span>
                      )}
                    </div>
                  )}

                {phase === 'waiting' && !status.storeListed && (
                  <p className={cn('mt-1.5 leading-relaxed text-fg-muted', bodySize)}>
                    {t('settings.linkStorePending')}
                  </p>
                )}

                {/* A quiet button has no fill to line up, so when one leads the
                    row it is pulled out by its padding and its label starts
                    where the text above it does. */}
                <div className={cn('mt-3 flex flex-wrap items-center gap-2', !leadAction && '-ml-3')}>
                  {phase === 'broken' && (
                    <Button
                      size="sm"
                      variant="primary"
                      loading={working}
                      onClick={() => void repair()}
                    >
                      {t('settings.linkRepair')}
                    </Button>
                  )}
                  {phase === 'waiting' && status.storeListed && (
                    <Button
                      size="sm"
                      variant="primary"
                      iconRight={<ExternalLink size={13} />}
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
                    icon={copied ? <Check size={13} /> : <Copy size={13} />}
                    onClick={() => void copyDiagnostics()}
                  >
                    {t('settings.linkCopyDiagnostics')}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={working}
                    onClick={() => void disconnect()}
                  >
                    {t('settings.linkDisconnect')}
                  </Button>
                  {/* The check is for the eye; this is the same news for a
                      screen reader. */}
                  {copied && (
                    <span role="status" className="sr-only">
                      {t('settings.linkDiagnosticsCopied')}
                    </span>
                  )}
                </div>

                {problem && (
                  <InlineNotice tone="error" className="mt-2">
                    {t(problem)}
                  </InlineNotice>
                )}

                <p className={cn('mt-2 leading-relaxed text-fg-muted', bodySize)}>
                  {t('settings.linkDiagnosticsHint')}
                </p>
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
              <TriangleAlert size={14} aria-hidden="true" className="shrink-0 text-warning" />
              {t('settings.linkSecondAccount')}
            </span>
          }
          description={t('settings.linkSecondAccountHint')}
        />
      )}
    </SettingGroup>
  );
}
