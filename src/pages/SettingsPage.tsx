import { motion } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { openPath } from '@tauri-apps/plugin-opener';
import {
  ChevronLeft,
  ChevronRight,
  FolderOpen,
  Gauge,
  Keyboard,
  Palette,
  ScrollText,
  SlidersHorizontal,
  Sparkles,
  Trash2,
} from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { HotkeyRecorder } from '@/components/settings/HotkeyRecorder';
import { ToolCard } from '@/components/settings/ToolCard';
import { Button } from '@/components/ui/Button';
import { Dropdown } from '@/components/ui/Dropdown';
import { Modal } from '@/components/ui/Modal';
import { Segmented } from '@/components/ui/Segmented';
import { SettingGroup, SettingRow, ToggleRow } from '@/components/ui/SettingRow';
import { Slider } from '@/components/ui/Slider';
import { TextInput } from '@/components/ui/TextInput';
import { Tooltip } from '@/components/ui/Tooltip';
import { LANGUAGES, useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { decodeQuality, encodeQuality } from '@/lib/downloadOptions';
import { formatBytes, truncateMiddle } from '@/lib/format';
import { SPRING, T } from '@/lib/motion';
import { IS_MOBILE } from '@/lib/platform';
import * as ipc from '@/services/ipc';
import { useSettingsStore } from '@/stores/useSettingsStore';
import { useToastStore } from '@/stores/useToastStore';
import { useToolsStore } from '@/stores/useToolsStore';
import type {
  CacheStats,
  DownloadMode,
  HotkeyAction,
  LanguageCode,
  Settings,
  ThemePreference,
} from '@/types';

export type SettingsSection =
  | 'general'
  | 'downloads'
  | 'appearance'
  | 'performance'
  | 'shortcuts'
  | 'advanced';

const ALL_SECTIONS: {
  id: SettingsSection;
  label: TranslationKey;
  summary: TranslationKey;
  icon: typeof Sparkles;
}[] = [
  { id: 'general', label: 'settings.general', summary: 'settings.generalSummary', icon: Sparkles },
  {
    id: 'downloads',
    label: 'settings.downloads',
    summary: 'settings.downloadsSummary',
    icon: FolderOpen,
  },
  {
    id: 'appearance',
    label: 'settings.appearance',
    summary: 'settings.appearanceSummary',
    icon: Palette,
  },
  {
    id: 'performance',
    label: 'settings.performance',
    summary: 'settings.performanceSummary',
    icon: Gauge,
  },
  { id: 'shortcuts', label: 'settings.hotkeys', summary: 'settings.hotkeys', icon: Keyboard },
  {
    id: 'advanced',
    label: 'settings.advanced',
    summary: 'settings.advancedSummary',
    icon: SlidersHorizontal,
  },
];

/** Keyboard shortcuts mean nothing without a keyboard. */
const SECTIONS = IS_MOBILE
  ? ALL_SECTIONS.filter((entry) => entry.id !== 'shortcuts')
  : ALL_SECTIONS;

/** Mirrors the defaults in `settings.rs`, for the per-hotkey reset button. */
const DEFAULT_HOTKEYS: Record<HotkeyAction, string> = {
  pasteUrl: 'Ctrl+V',
  download: 'Ctrl+Enter',
  openDownloads: 'Ctrl+Shift+D',
  openHistory: 'Ctrl+Shift+H',
  openSettings: 'Ctrl+,',
};

/**
 * The heading of a section's first group. On a phone the section's own page
 * title already says it, one line above.
 */
function leadTitle(title: string): string | undefined {
  return IS_MOBILE ? undefined : title;
}

/** A control's width beside its label on the desktop; a phone gives it the row. */
function controlWidth(desktop: string): string {
  return IS_MOBILE ? 'w-full' : desktop;
}

interface SettingsPageProps {
  settings: Settings;
  /**
   * On a phone, the section open on its own page, or null for the list of
   * sections. Owned by the app so the Back gesture can close it.
   */
  section?: SettingsSection | null;
  onSectionChange?: (section: SettingsSection | null) => void;
}

export function SettingsPage({ settings, section: openSection, onSectionChange }: SettingsPageProps) {
  const { t } = useTranslation();
  const update = useSettingsStore((state) => state.update);
  const reset = useSettingsStore((state) => state.reset);
  const [desktopSection, setDesktopSection] = useState<SettingsSection>('general');
  const [confirmReset, setConfirmReset] = useState(false);

  const section = IS_MOBILE ? (openSection ?? null) : desktopSection;
  const entry = SECTIONS.find((candidate) => candidate.id === section);

  const content = section && (
    <motion.div
      key={section}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={T.component}
      className="min-w-0 flex-1 space-y-6"
    >
      {section === 'general' && <GeneralSection settings={settings} update={update} />}
      {section === 'downloads' && <DownloadsSection settings={settings} update={update} />}
      {section === 'appearance' && <AppearanceSection settings={settings} update={update} />}
      {section === 'performance' && <PerformanceSection settings={settings} update={update} />}
      {section === 'shortcuts' && <ShortcutsSection settings={settings} update={update} />}
      {section === 'advanced' && (
        <AdvancedSection
          settings={settings}
          update={update}
          onRequestReset={() => setConfirmReset(true)}
        />
      )}
    </motion.div>
  );

  const resetModal = (
    <Modal
      open={confirmReset}
      onClose={() => setConfirmReset(false)}
      title={t('settings.resetConfirm')}
      closeLabel={t('common.close')}
      footer={
        <>
          <Button variant="ghost" onClick={() => setConfirmReset(false)}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="danger"
            data-autofocus
            onClick={async () => {
              await reset();
              setConfirmReset(false);
            }}
          >
            {t('settings.resetSettings')}
          </Button>
        </>
      }
    />
  );

  if (IS_MOBILE) {
    // A phone shows the sections as a list of large rows, and each one opens
    // on a page of its own -- the pattern of the phone's own Settings app. A
    // strip of small tabs scrolling sideways was hard to hit, and easy to
    // scroll when a tap was meant.
    return (
      <div className="mx-auto flex w-full max-w-[880px] flex-col px-4 pb-12">
        {entry ? (
          <>
            <div className="sticky top-0 z-10 -mx-4 mb-2 flex h-14 items-center gap-1 bg-bg px-1.5">
              <button
                type="button"
                onClick={() => onSectionChange?.(null)}
                aria-label={t('common.back')}
                className="pressable-sm flex size-11 shrink-0 items-center justify-center rounded-full text-fg active:bg-surface-active"
              >
                <ChevronLeft size={24} />
              </button>
              <h2 className="truncate text-[17px] font-semibold text-fg">{t(entry.label)}</h2>
            </div>
            {content}
          </>
        ) : (
          <motion.nav
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            transition={T.component}
            aria-label={t('settings.title')}
            className="mt-4 overflow-hidden rounded-[var(--radius-panel)] border border-[var(--border)] bg-surface divide-y divide-[var(--border)]"
          >
            {SECTIONS.map((candidate) => {
              const Icon = candidate.icon;
              return (
                <button
                  key={candidate.id}
                  type="button"
                  onClick={() => onSectionChange?.(candidate.id)}
                  className="flex min-h-[72px] w-full items-center gap-3.5 px-4 py-3 text-left transition-colors duration-150 ease-out-quint active:bg-surface-active"
                >
                  <span className="flex size-10 shrink-0 items-center justify-center rounded-[10px] bg-accent-soft text-accent">
                    <Icon size={19} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-[15.5px] font-medium text-fg">
                      {t(candidate.label)}
                    </span>
                    <span className="mt-0.5 block truncate text-[13px] text-fg-muted">
                      {t(candidate.summary)}
                    </span>
                  </span>
                  <ChevronRight size={19} className="shrink-0 text-fg-faint" />
                </button>
              );
            })}
          </motion.nav>
        )}
        {resetModal}
      </div>
    );
  }

  return (
    <div className="mx-auto flex w-full max-w-[880px] gap-6 px-6 pb-12">
      <nav className="sticky top-3 h-fit w-[168px] shrink-0 space-y-0.5" aria-label={t('settings.title')}>
        {SECTIONS.map((candidate) => {
          const Icon = candidate.icon;
          const active = section === candidate.id;
          return (
            <button
              key={candidate.id}
              type="button"
              onClick={() => setDesktopSection(candidate.id)}
              className={cn(
                'relative flex h-8.5 w-full items-center gap-2.5 rounded-lg px-2.5 text-[13px] font-medium',
                'transition-colors duration-150 ease-out-quint',
                active ? 'text-fg' : 'text-fg-muted hover:bg-surface-hover hover:text-fg',
              )}
            >
              {active && (
                <motion.span
                  layoutId="settings-active"
                  transition={SPRING.glide}
                  className="absolute inset-0 rounded-lg bg-surface-active"
                />
              )}
              <Icon size={15} className={cn('relative z-10', active && 'text-accent')} />
              <span className="relative z-10">{t(candidate.label)}</span>
            </button>
          );
        })}
      </nav>

      {content}
      {resetModal}
    </div>
  );
}

type UpdateFn = (patch: Partial<Settings>) => Promise<void>;

function GeneralSection({ settings, update }: { settings: Settings; update: UpdateFn }) {
  const { t } = useTranslation();

  return (
    <>
      <SettingGroup title={leadTitle(t('settings.general'))}>
        {!IS_MOBILE && (
          <>
            <ToggleRow
              title={t('settings.startWithWindows')}
              description={t('settings.startWithWindowsHint')}
              checked={settings.startWithWindows}
              onChange={(value) => void update({ startWithWindows: value })}
            />
            <ToggleRow
              title={t('settings.minimizeToTray')}
              description={t('settings.minimizeToTrayHint')}
              checked={settings.minimizeToTray}
              onChange={(value) => void update({ minimizeToTray: value })}
            />
            <ToggleRow
              title={t('settings.closeToTray')}
              description={t('settings.closeToTrayHint')}
              checked={settings.closeToTray}
              onChange={(value) => void update({ closeToTray: value })}
            />
          </>
        )}
        <ToggleRow
          title={t('settings.clipboard')}
          description={t('settings.clipboardHint')}
          checked={settings.clipboardMonitoring}
          onChange={(value) => void update({ clipboardMonitoring: value })}
        />
      </SettingGroup>

      <SettingGroup title={t('settings.notifications')}>
        <ToggleRow
          title={t('settings.notifications')}
          description={t('settings.notificationsHint')}
          checked={settings.notificationsEnabled}
          onChange={(value) => void update({ notificationsEnabled: value })}
        />
        <ToggleRow
          title={t('settings.notifyComplete')}
          checked={settings.notifyOnComplete}
          disabled={!settings.notificationsEnabled}
          onChange={(value) => void update({ notifyOnComplete: value })}
        />
        <ToggleRow
          title={t('settings.notifyError')}
          checked={settings.notifyOnError}
          disabled={!settings.notificationsEnabled}
          onChange={(value) => void update({ notifyOnError: value })}
        />
      </SettingGroup>
    </>
  );
}

function DownloadsSection({ settings, update }: { settings: Settings; update: UpdateFn }) {
  const { t } = useTranslation();
  const [template, setTemplate] = useState(settings.filenameTemplate);
  const [preview, setPreview] = useState('');

  useEffect(() => setTemplate(settings.filenameTemplate), [settings.filenameTemplate]);

  // The preview comes from the same renderer the downloader uses, so what is
  // shown here is exactly the name a file will get.
  useEffect(() => {
    let active = true;
    ipc.previewFilename(template).then((value) => {
      if (active) setPreview(value);
    });
    return () => {
      active = false;
    };
  }, [template]);

  const pickFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: settings.downloadDir,
    });
    if (typeof selected === 'string') void update({ downloadDir: selected });
  };

  return (
    <>
      <SettingGroup title={leadTitle(t('settings.downloads'))}>
        <SettingRow
          title={t('settings.downloadDir')}
          description={
            IS_MOBILE ? (
              <>
                <span className="block font-mono text-[11.5px] [overflow-wrap:anywhere]">
                  {settings.downloadDir}
                </span>
                <span className="mt-1 block">{t('settings.downloadDirMobileHint')}</span>
              </>
            ) : (
              <Tooltip label={settings.downloadDir}>
                <span className="font-mono text-[11.5px]">
                  {truncateMiddle(settings.downloadDir, 52)}
                </span>
              </Tooltip>
            )
          }
          control={
            // The Android folder picker hands back a document tree, not a
            // path the downloader could write to, so the folder is fixed there.
            IS_MOBILE ? undefined : (
              <Button size="sm" variant="secondary" onClick={pickFolder}>
                {t('settings.browse')}
              </Button>
            )
          }
        />

        <SettingRow
          title={t('settings.defaultMode')}
          stacked={IS_MOBILE}
          control={
            <div className={controlWidth('w-[220px]')}>
              <Segmented
                value={settings.defaultMode}
                options={[
                  { value: 'video', label: t('options.modeVideo') },
                  { value: 'audio', label: t('options.modeAudio') },
                ]}
                onChange={(mode) => void update({ defaultMode: mode as DownloadMode })}
                size="sm"
              />
            </div>
          }
        />

        <SettingRow
          title={t('settings.defaultQuality')}
          stacked={IS_MOBILE}
          control={
            <div className={controlWidth('w-[200px]')}>
              <Dropdown
                value={encodeQuality(settings.defaultQuality)}
                options={[
                  { value: 'best', label: t('options.qualityBest') },
                  { value: 'auto', label: t('options.qualityAuto') },
                  { value: 'h:2160', label: '2160p' },
                  { value: 'h:1440', label: '1440p' },
                  { value: 'h:1080', label: '1080p' },
                  { value: 'h:720', label: '720p' },
                  { value: 'h:480', label: '480p' },
                  { value: 'h:360', label: '360p' },
                ]}
                onChange={(key) => void update({ defaultQuality: decodeQuality(key) })}
              />
            </div>
          }
        />

        <SettingRow
          title={t('settings.defaultContainer')}
          stacked={IS_MOBILE}
          control={
            <div className={controlWidth('w-[200px]')}>
              <Dropdown
                value={settings.defaultContainer ?? ''}
                options={[
                  { value: '', label: t('settings.containerKeep') },
                  { value: 'mp4', label: 'MP4' },
                  { value: 'mkv', label: 'MKV' },
                  { value: 'webm', label: 'WEBM' },
                ]}
                onChange={(value) => void update({ defaultContainer: value || null })}
              />
            </div>
          }
        />

        <SettingRow
          title={t('settings.maxConcurrent')}
          description={t('settings.maxConcurrentHint')}
          stacked={IS_MOBILE}
          control={
            <div className={controlWidth('w-[180px]')}>
              <Slider
                value={settings.maxConcurrentDownloads}
                min={1}
                max={10}
                onChange={(value) => void update({ maxConcurrentDownloads: value })}
                formatValue={(value) => String(value)}
              />
            </div>
          }
        />

        <SettingRow
          title={t('settings.autoRetry')}
          description={t('settings.autoRetryHint')}
          stacked={IS_MOBILE}
          control={
            <div className={controlWidth('w-[180px]')}>
              <Slider
                value={settings.autoRetryCount}
                min={0}
                max={10}
                onChange={(value) => void update({ autoRetryCount: value })}
                formatValue={(value) => String(value)}
              />
            </div>
          }
        />

        <SettingRow
          title={t('settings.filenameTemplate')}
          description={t('settings.filenameTemplateHint')}
          stacked
          control={
            <div className="space-y-2">
              <TextInput
                value={template}
                monospace
                onChange={(event) => setTemplate(event.target.value)}
                onBlur={() => void update({ filenameTemplate: template })}
                aria-label={t('settings.filenameTemplate')}
              />
              <p className="truncate font-mono text-[11.5px] text-fg-faint">
                {t('settings.filenamePreview')}: {preview}
              </p>
            </div>
          }
        />
      </SettingGroup>
    </>
  );
}

function AppearanceSection({ settings, update }: { settings: Settings; update: UpdateFn }) {
  const { t } = useTranslation();

  return (
    <SettingGroup title={leadTitle(t('settings.appearance'))}>
      <SettingRow
        title={t('settings.theme')}
        stacked={IS_MOBILE}
        control={
          <div className={controlWidth('w-[260px]')}>
            <Segmented
              value={settings.theme}
              options={[
                { value: 'dark', label: t('topbar.themeDark') },
                { value: 'light', label: t('topbar.themeLight') },
                { value: 'system', label: t('topbar.themeSystem') },
              ]}
              onChange={(theme) => void update({ theme: theme as ThemePreference })}
              size="sm"
            />
          </div>
        }
      />
      <SettingRow
        title={t('settings.language')}
        stacked={IS_MOBILE}
        control={
          <div className={controlWidth('w-[180px]')}>
            <Dropdown
              value={settings.language}
              options={LANGUAGES.map((entry) => ({ value: entry.code, label: entry.label }))}
              onChange={(language) => void update({ language: language as LanguageCode })}
            />
          </div>
        }
      />
      <ToggleRow
        title={t('settings.reduceMotion')}
        description={t('settings.reduceMotionHint')}
        checked={settings.reduceMotion}
        onChange={(value) => void update({ reduceMotion: value })}
      />
      {/* A phone's backdrop stands still (see Background), so there is no
          animation there to turn off. */}
      {!IS_MOBILE && (
        <ToggleRow
          title={t('settings.animatedBackground')}
          description={t('settings.animatedBackgroundHint')}
          checked={settings.showAnimatedBackground}
          onChange={(value) => void update({ showAnimatedBackground: value })}
        />
      )}
    </SettingGroup>
  );
}

function PerformanceSection({ settings, update }: { settings: Settings; update: UpdateFn }) {
  const { t } = useTranslation();
  const pushToast = useToastStore((state) => state.push);
  const [stats, setStats] = useState<CacheStats | null>(null);

  const refreshStats = useCallback(() => {
    ipc.cacheStats().then(setStats).catch(() => setStats(null));
  }, []);

  useEffect(refreshStats, [refreshStats]);

  return (
    <SettingGroup title={leadTitle(t('settings.performance'))}>
      <ToggleRow
        title={t('settings.lowResource')}
        description={t('settings.lowResourceHint')}
        checked={settings.lowResourceMode}
        onChange={(value) => void update({ lowResourceMode: value })}
      />
      {/* The GPU pass is an NVIDIA encoder, which no phone has. */}
      {!IS_MOBILE && (
        <ToggleRow
          title={t('settings.hardwareAcceleration')}
          description={t('settings.hardwareAccelerationHint')}
          checked={settings.hardwareAcceleration}
          onChange={(value) => void update({ hardwareAcceleration: value })}
        />
      )}
      <SettingRow
        title={t('settings.cacheLimit')}
        stacked={IS_MOBILE}
        description={
          stats
            ? t('settings.cacheUsage', {
                used: formatBytes(stats.totalBytes),
                limit: formatBytes(settings.cacheLimitMb * 1024 * 1024),
              })
            : undefined
        }
        control={
          <div className={controlWidth('w-[200px]')}>
            <Slider
              value={settings.cacheLimitMb}
              min={32}
              max={2048}
              step={32}
              onChange={(value) => void update({ cacheLimitMb: value })}
              formatValue={(value) => `${value} MB`}
            />
          </div>
        }
      />
      <SettingRow
        title={t('settings.cacheTitle')}
        description={t('settings.cacheHint')}
        control={
          <Button
            size="sm"
            variant="secondary"
            icon={<Trash2 size={13} />}
            onClick={async () => {
              await ipc.clearCache();
              refreshStats();
              pushToast({ tone: 'success', title: t('settings.cacheCleared') });
            }}
          >
            {t('settings.clearCache')}
          </Button>
        }
      />
    </SettingGroup>
  );
}

function ShortcutsSection({ settings, update }: { settings: Settings; update: UpdateFn }) {
  const { t } = useTranslation();
  const actions = Object.keys(DEFAULT_HOTKEYS) as HotkeyAction[];

  const findConflict = useCallback(
    (accelerator: string) => {
      const clash = actions.find((action) => settings.hotkeys[action] === accelerator);
      return clash ? t(`hotkey.${clash}` as TranslationKey) : null;
    },
    [actions, settings.hotkeys, t],
  );

  return (
    <SettingGroup title={t('settings.hotkeys')}>
      {actions.map((action) => (
        <SettingRow
          key={action}
          title={t(`hotkey.${action}` as TranslationKey)}
          control={
            <HotkeyRecorder
              value={settings.hotkeys[action] ?? DEFAULT_HOTKEYS[action]}
              defaultValue={DEFAULT_HOTKEYS[action]}
              findConflict={findConflict}
              onChange={(accelerator) =>
                void update({ hotkeys: { ...settings.hotkeys, [action]: accelerator } })
              }
            />
          }
        />
      ))}
    </SettingGroup>
  );
}

function AdvancedSection({
  settings,
  update,
  onRequestReset,
}: {
  settings: Settings;
  update: UpdateFn;
  onRequestReset: () => void;
}) {
  const { t } = useTranslation();
  const tools = useToolsStore((state) => state.tools);
  const installing = useToolsStore((state) => state.installing);
  const install = useToolsStore((state) => state.install);
  const pushToast = useToastStore((state) => state.push);

  const [proxy, setProxy] = useState(settings.proxyUrl ?? '');
  const [userAgent, setUserAgent] = useState(settings.customUserAgent ?? '');

  useEffect(() => setProxy(settings.proxyUrl ?? ''), [settings.proxyUrl]);
  useEffect(() => setUserAgent(settings.customUserAgent ?? ''), [settings.customUserAgent]);

  const runInstall = async (tool: 'engine' | 'ffmpeg') => {
    const ok = await install(tool);
    pushToast(
      ok
        ? { tone: 'success', title: t('common.done') }
        : {
            tone: 'error',
            title: t('settings.toolInstallFailed'),
            body: useToolsStore.getState().error ?? t('error.network.message'),
            durationMs: 9000,
          },
    );
  };

  const engine = tools?.engine ?? {
    name: 'engine' as const,
    available: false,
    path: null,
    version: null,
    source: 'missing' as const,
  };
  const ffmpeg = tools?.ffmpeg ?? {
    name: 'ffmpeg' as const,
    available: false,
    path: null,
    version: null,
    source: 'missing' as const,
  };

  return (
    <>
      <SettingGroup title={t('about.externalTools')}>
        <ToolCard
          status={engine}
          titleKey="settings.engine"
          hintKey="settings.engineHint"
          installing={installing.engine}
          customPath={settings.enginePath}
          onInstall={() => void runInstall('engine')}
          onLocate={(path) => void update({ enginePath: path })}
          onResetPath={() => void update({ enginePath: null })}
        />
        <ToolCard
          status={ffmpeg}
          titleKey="settings.ffmpeg"
          hintKey="settings.ffmpegHint"
          installing={installing.ffmpeg}
          customPath={settings.ffmpegPath}
          optionalNoteKey="setup.ffmpegOptional"
          onInstall={() => void runInstall('ffmpeg')}
          onLocate={(path) => void update({ ffmpegPath: path })}
          onResetPath={() => void update({ ffmpegPath: null })}
        />
      </SettingGroup>

      <SettingGroup title={t('settings.advanced')}>
        <SettingRow
          title={t('settings.networkTimeout')}
          stacked={IS_MOBILE}
          control={
            <div className={controlWidth('w-[200px]')}>
              <Slider
                value={settings.networkTimeoutSec}
                min={5}
                max={180}
                step={5}
                onChange={(value) => void update({ networkTimeoutSec: value })}
                formatValue={(value) => t('common.seconds', { n: value })}
              />
            </div>
          }
        />
        <SettingRow
          title={t('settings.proxy')}
          stacked
          control={
            <TextInput
              value={proxy}
              monospace
              placeholder={t('settings.proxyPlaceholder')}
              onChange={(event) => setProxy(event.target.value)}
              onBlur={() => void update({ proxyUrl: proxy.trim() || null })}
              aria-label={t('settings.proxy')}
            />
          }
        />
        <SettingRow
          title={t('settings.userAgent')}
          stacked
          control={
            <TextInput
              value={userAgent}
              monospace
              placeholder={t('settings.userAgentPlaceholder')}
              onChange={(event) => setUserAgent(event.target.value)}
              onBlur={() => void update({ customUserAgent: userAgent.trim() || null })}
              aria-label={t('settings.userAgent')}
            />
          }
        />
        <ToggleRow
          title={t('settings.debugLogging')}
          description={t('settings.debugLoggingHint')}
          checked={settings.debugLogging}
          onChange={(value) => void update({ debugLogging: value })}
        />
        {!IS_MOBILE && (
          <SettingRow
            title={t('settings.logs')}
            description={t('settings.logsHint')}
            control={
              <Button
                size="sm"
                variant="secondary"
                icon={<ScrollText size={13} />}
                onClick={async () => {
                  const dir = await ipc.getLogDir();
                  await openPath(dir);
                }}
              >
                {t('settings.openLogs')}
              </Button>
            }
          />
        )}
        <SettingRow
          title={t('settings.resetTitle')}
          description={t('settings.resetHint')}
          control={
            <Button size="sm" variant="danger" onClick={onRequestReset}>
              {t('settings.resetSettings')}
            </Button>
          }
        />
      </SettingGroup>
    </>
  );
}

export { DEFAULT_HOTKEYS };
