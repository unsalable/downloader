import { motion } from 'motion/react';
import { open } from '@tauri-apps/plugin-dialog';
import { openPath } from '@tauri-apps/plugin-opener';
import {
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
import { SettingGroup, SettingRow } from '@/components/ui/SettingRow';
import { Slider } from '@/components/ui/Slider';
import { TextInput } from '@/components/ui/TextInput';
import { Toggle } from '@/components/ui/Toggle';
import { Tooltip } from '@/components/ui/Tooltip';
import { LANGUAGES, useTranslation } from '@/i18n';
import type { TranslationKey } from '@/i18n';
import { cn } from '@/lib/cn';
import { decodeQuality, encodeQuality } from '@/lib/downloadOptions';
import { formatBytes, truncateMiddle } from '@/lib/format';
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

type SectionId = 'general' | 'downloads' | 'appearance' | 'performance' | 'shortcuts' | 'advanced';

const SECTIONS: { id: SectionId; label: TranslationKey; icon: typeof Sparkles }[] = [
  { id: 'general', label: 'settings.general', icon: Sparkles },
  { id: 'downloads', label: 'settings.downloads', icon: FolderOpen },
  { id: 'appearance', label: 'settings.appearance', icon: Palette },
  { id: 'performance', label: 'settings.performance', icon: Gauge },
  { id: 'shortcuts', label: 'settings.hotkeys', icon: Keyboard },
  { id: 'advanced', label: 'settings.advanced', icon: SlidersHorizontal },
];

/** Mirrors the defaults in `settings.rs`, for the per-hotkey reset button. */
const DEFAULT_HOTKEYS: Record<HotkeyAction, string> = {
  pasteUrl: 'Ctrl+V',
  download: 'Ctrl+Enter',
  openDownloads: 'Ctrl+Shift+D',
  openHistory: 'Ctrl+Shift+H',
  openSettings: 'Ctrl+,',
};

export function SettingsPage({ settings }: { settings: Settings }) {
  const { t } = useTranslation();
  const update = useSettingsStore((state) => state.update);
  const reset = useSettingsStore((state) => state.reset);
  const [section, setSection] = useState<SectionId>('general');
  const [confirmReset, setConfirmReset] = useState(false);

  return (
    <div className="mx-auto flex w-full max-w-[880px] gap-6 px-6 pb-12">
      <nav className="sticky top-3 h-fit w-[168px] shrink-0 space-y-0.5" aria-label={t('settings.title')}>
        {SECTIONS.map((entry) => {
          const Icon = entry.icon;
          const active = section === entry.id;
          return (
            <button
              key={entry.id}
              type="button"
              onClick={() => setSection(entry.id)}
              className={cn(
                'relative flex h-8.5 w-full items-center gap-2.5 rounded-lg px-2.5 text-[13px] font-medium',
                'transition-colors duration-150',
                active ? 'text-fg' : 'text-fg-muted hover:bg-surface-hover hover:text-fg',
              )}
            >
              {active && (
                <motion.span
                  layoutId="settings-active"
                  transition={{ type: 'spring', stiffness: 480, damping: 40 }}
                  className="absolute inset-0 rounded-lg bg-surface-active"
                />
              )}
              <Icon size={15} className={cn('relative z-10', active && 'text-accent')} />
              <span className="relative z-10">{t(entry.label)}</span>
            </button>
          );
        })}
      </nav>

      <motion.div
        key={section}
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
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
    </div>
  );
}

type UpdateFn = (patch: Partial<Settings>) => Promise<void>;

function GeneralSection({ settings, update }: { settings: Settings; update: UpdateFn }) {
  const { t } = useTranslation();

  return (
    <>
      <SettingGroup title={t('settings.general')}>
        <SettingRow
          title={t('settings.startWithWindows')}
          description={t('settings.startWithWindowsHint')}
          control={
            <Toggle
              checked={settings.startWithWindows}
              onChange={(value) => void update({ startWithWindows: value })}
              label={t('settings.startWithWindows')}
            />
          }
        />
        <SettingRow
          title={t('settings.minimizeToTray')}
          description={t('settings.minimizeToTrayHint')}
          control={
            <Toggle
              checked={settings.minimizeToTray}
              onChange={(value) => void update({ minimizeToTray: value })}
              label={t('settings.minimizeToTray')}
            />
          }
        />
        <SettingRow
          title={t('settings.closeToTray')}
          description={t('settings.closeToTrayHint')}
          control={
            <Toggle
              checked={settings.closeToTray}
              onChange={(value) => void update({ closeToTray: value })}
              label={t('settings.closeToTray')}
            />
          }
        />
        <SettingRow
          title={t('settings.clipboard')}
          description={t('settings.clipboardHint')}
          control={
            <Toggle
              checked={settings.clipboardMonitoring}
              onChange={(value) => void update({ clipboardMonitoring: value })}
              label={t('settings.clipboard')}
            />
          }
        />
      </SettingGroup>

      <SettingGroup title={t('settings.notifications')}>
        <SettingRow
          title={t('settings.notifications')}
          description={t('settings.notificationsHint')}
          control={
            <Toggle
              checked={settings.notificationsEnabled}
              onChange={(value) => void update({ notificationsEnabled: value })}
              label={t('settings.notifications')}
            />
          }
        />
        <SettingRow
          title={t('settings.notifyComplete')}
          control={
            <Toggle
              checked={settings.notifyOnComplete}
              disabled={!settings.notificationsEnabled}
              onChange={(value) => void update({ notifyOnComplete: value })}
              label={t('settings.notifyComplete')}
            />
          }
        />
        <SettingRow
          title={t('settings.notifyError')}
          control={
            <Toggle
              checked={settings.notifyOnError}
              disabled={!settings.notificationsEnabled}
              onChange={(value) => void update({ notifyOnError: value })}
              label={t('settings.notifyError')}
            />
          }
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
      <SettingGroup title={t('settings.downloads')}>
        <SettingRow
          title={t('settings.downloadDir')}
          description={
            <Tooltip label={settings.downloadDir}>
              <span className="font-mono text-[11.5px]">
                {truncateMiddle(settings.downloadDir, 52)}
              </span>
            </Tooltip>
          }
          control={
            <Button size="sm" variant="secondary" onClick={pickFolder}>
              {t('settings.browse')}
            </Button>
          }
        />

        <SettingRow
          title={t('settings.defaultMode')}
          control={
            <div className="w-[220px]">
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
          control={
            <div className="w-[200px]">
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
          control={
            <div className="w-[200px]">
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
          control={
            <div className="w-[180px]">
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
          control={
            <div className="w-[180px]">
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
    <SettingGroup title={t('settings.appearance')}>
      <SettingRow
        title={t('settings.theme')}
        control={
          <div className="w-[260px]">
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
        control={
          <div className="w-[180px]">
            <Dropdown
              value={settings.language}
              options={LANGUAGES.map((entry) => ({ value: entry.code, label: entry.label }))}
              onChange={(language) => void update({ language: language as LanguageCode })}
            />
          </div>
        }
      />
      <SettingRow
        title={t('settings.reduceMotion')}
        description={t('settings.reduceMotionHint')}
        control={
          <Toggle
            checked={settings.reduceMotion}
            onChange={(value) => void update({ reduceMotion: value })}
            label={t('settings.reduceMotion')}
          />
        }
      />
      <SettingRow
        title={t('settings.animatedBackground')}
        description={t('settings.animatedBackgroundHint')}
        control={
          <Toggle
            checked={settings.showAnimatedBackground}
            onChange={(value) => void update({ showAnimatedBackground: value })}
            label={t('settings.animatedBackground')}
          />
        }
      />
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
    <SettingGroup title={t('settings.performance')}>
      <SettingRow
        title={t('settings.lowResource')}
        description={t('settings.lowResourceHint')}
        control={
          <Toggle
            checked={settings.lowResourceMode}
            onChange={(value) => void update({ lowResourceMode: value })}
            label={t('settings.lowResource')}
          />
        }
      />
      <SettingRow
        title={t('settings.hardwareAcceleration')}
        description={t('settings.hardwareAccelerationHint')}
        control={
          <Toggle
            checked={settings.hardwareAcceleration}
            onChange={(value) => void update({ hardwareAcceleration: value })}
            label={t('settings.hardwareAcceleration')}
          />
        }
      />
      <SettingRow
        title={t('settings.cacheLimit')}
        description={
          stats
            ? t('settings.cacheUsage', {
                used: formatBytes(stats.totalBytes),
                limit: formatBytes(settings.cacheLimitMb * 1024 * 1024),
              })
            : undefined
        }
        control={
          <div className="w-[200px]">
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
          control={
            <div className="w-[200px]">
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
        <SettingRow
          title={t('settings.debugLogging')}
          description={t('settings.debugLoggingHint')}
          control={
            <Toggle
              checked={settings.debugLogging}
              onChange={(value) => void update({ debugLogging: value })}
              label={t('settings.debugLogging')}
            />
          }
        />
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
