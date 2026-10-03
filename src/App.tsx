import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/api/dialog';
import { invoke } from '@tauri-apps/api/tauri';

type CatalogSummary = {
  version: number;
  catalogRevision: string;
  marketplaces: number;
  assets: number;
};

type GlobalSkill = {
  name: string;
  directory_location: string;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
};

type GlobalSkillsState = {
  skills: GlobalSkill[];
  scan_root: string;
  refreshed_at: string | null;
  error_message: string | null;
};

type GlobalInventoryAsset = {
  key: string;
  name: string;
  kind: 'agent' | 'skill';
  location: string;
  description: string | null;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
  source: 'global' | 'marketplace';
  parent: string | null;
};

type CatalogState = {
  summary: CatalogSummary;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
  source_repository: string | null;
  source_branch: string;
  refreshed_at: string | null;
  stale: boolean;
};

const fallbackGlobalSkillsState: GlobalSkillsState = {
  skills: [],
  scan_root: '.copilot\\skills',
  refreshed_at: null,
  error_message: null,
};

type MarketplaceOption = {
  id: string;
  name: string;
  repository: string;
  agents: MarketplaceAgent[];
  skills: MarketplaceSkill[];
  plugins: MarketplacePlugin[];
};

type MarketplaceAgent = {
  name: string;
  description: string | null;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
};

type MarketplacePlugin = {
  name: string;
  skills: MarketplaceSkill[];
};

type MarketplaceSkill = {
  name: string;
  directory_location: string;
};

type Profile = {
  id: string;
  name: string;
  project_location: string | null;
  description: string | null;
  enabled: boolean;
  version: string | null;
  catalogRevision: string | null;
  selected_assets: string[];
};

type ProfileAsset = {
  asset_id: string;
  kind: 'agent' | 'skill';
  marketplace_id: string;
  marketplace_name: string;
  name: string;
  description: string | null;
  plugin_name: string | null;
  destination: string;
  installation_state: 'Available' | 'Installed' | 'Modified' | 'Conflict';
  source_available: boolean;
};

type GlobalAgent = {
  name: string;
  file_location: string;
  description: string | null;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
};

type GlobalAgentsState = {
  agents: GlobalAgent[];
  scan_root: string;
  refreshed_at: string | null;
  error_message: string | null;
};

type InstalledMarketplace = {
  id: string;
  name: string;
  directory_location: string;
  repository: string | null;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
  plugins: InstalledPlugin[];
};

type InstalledPlugin = {
  name: string;
  directory_location: string;
  skills: InstalledSkill[];
  agents: InstalledPluginAgent[];
};

type InstalledPluginAgent = {
  name: string;
  file_location: string;
  description: string | null;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
};

type InstalledSkill = {
  name: string;
  directory_location: string;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
};

type InstalledMarketplacesState = {
  marketplaces: InstalledMarketplace[];
  scan_root: string;
  refreshed_at: string | null;
  error_message: string | null;
};

type ProjectProfileDefaults = {
  displayName: string;
};

type ProfileFormState = {
  id: string;
  name: string;
  project_location: string;
  description: string;
  enabled: boolean;
  version: string | null;
  catalogRevision: string | null;
  selected_assets: string[];
};

const fallbackSummary: CatalogSummary = {
  version: 1,
  catalogRevision: '8e6912531e2dc22d6c96dde0afd2e399eb2d9dd3',
  marketplaces: 1,
  assets: 0,
};

const fallbackDetails: CatalogState = {
  summary: fallbackSummary,
  trust_status: 'Trusted',
  source_repository: null,
  source_branch: 'main',
  refreshed_at: null,
  stale: true,
};

const fallbackMarketplaces: MarketplaceOption[] = [
  {
    id: 'awesome-copilot',
    name: 'awesome-copilot',
    repository: 'https://github.com/github/awesome-copilot',
    agents: [],
    skills: [],
    plugins: [],
  },
];

const fallbackProjectDefaults: ProjectProfileDefaults = {
  displayName: 'Project',
};

const fallbackGlobalAgentsState: GlobalAgentsState = {
  agents: [],
  scan_root: '.copilot\\agents',
  refreshed_at: null,
  error_message: null,
};

const fallbackInstalledMarketplacesState: InstalledMarketplacesState = {
  marketplaces: [],
  scan_root: '.copilot\\installed-plugins',
  refreshed_at: null,
  error_message: null,
};

function buildGlobalInventory(
  agentsState: GlobalAgentsState,
  skillsState: GlobalSkillsState,
  marketplacesState: InstalledMarketplacesState,
): GlobalInventoryAsset[] {
  const assets: GlobalInventoryAsset[] = [
    ...agentsState.agents.map((agent) => ({
      key: `agent:${agent.file_location}`,
      name: agent.name,
      kind: 'agent' as const,
      location: agent.file_location,
      description: agent.description,
      trust_status: agent.trust_status,
      source: 'global' as const,
      parent: null,
    })),
    ...skillsState.skills.map((skill) => ({
      key: `skill:${skill.directory_location}`,
      name: skill.name,
      kind: 'skill' as const,
      location: skill.directory_location,
      description: null,
      trust_status: skill.trust_status,
      source: 'global' as const,
      parent: null,
    })),
    ...marketplacesState.marketplaces.flatMap((marketplace) =>
      marketplace.plugins.flatMap((plugin) => [
        ...plugin.agents.map((agent) => ({
          key: `agent:${agent.file_location}`,
          name: agent.name,
          kind: 'agent' as const,
          location: agent.file_location,
          description: agent.description,
          trust_status: agent.trust_status,
          source: 'marketplace' as const,
          parent: `${marketplace.name} / ${plugin.name}`,
        })),
        ...plugin.skills.map((skill) => ({
          key: `skill:${skill.directory_location}`,
          name: skill.name,
          kind: 'skill' as const,
          location: skill.directory_location,
          description: null,
          trust_status: skill.trust_status,
          source: 'marketplace' as const,
          parent: `${marketplace.name} / ${plugin.name}`,
        })),
      ]),
    ),
  ];
  return assets.sort((left, right) =>
    left.name.toLocaleLowerCase().localeCompare(right.name.toLocaleLowerCase()),
  );
}

function normalizeOptionalText(value: string) {
  const normalized = value.trim();
  return normalized.length === 0 ? null : normalized;
}

function createProfileForm(
  displayName: string,
  catalogRevision: string | null,
): ProfileFormState {
  return {
    id: '',
    name: displayName,
    project_location: '',
    description: '',
    enabled: true,
    version: '1',
    catalogRevision,
    selected_assets: [],
  };
}

function profileToForm(profile: Profile): ProfileFormState {
  return {
    id: profile.id,
    name: profile.name,
    project_location: profile.project_location ?? '',
    description: profile.description ?? '',
    enabled: profile.enabled,
    version: profile.version,
    catalogRevision: profile.catalogRevision,
    selected_assets: profile.selected_assets,
  };
}

function guessProjectName(projectLocation: string) {
  const segments = projectLocation
    .split(/[\\/]/)
    .map((segment) => segment.trim())
    .filter(Boolean);
  return segments.length === 0 ? null : segments[segments.length - 1];
}

export default function App() {
  const [isLoading, setIsLoading] = useState(true);
  const startupLoadStarted = useRef(false);
  const [summary, setSummary] = useState<CatalogSummary>(fallbackSummary);
  const [status, setStatus] = useState('Trusted marketplaces are ready.');
  const [details, setDetails] = useState<CatalogState | null>(null);
  const [marketplaces, setMarketplaces] = useState<MarketplaceOption[]>([]);
  const [marketplaceAgentFilters, setMarketplaceAgentFilters] = useState<Record<string, string>>(
    {},
  );
  const [marketplaceSkillFilters, setMarketplaceSkillFilters] = useState<Record<string, string>>(
    {},
  );
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [installedMarketplacesState, setInstalledMarketplacesState] =
    useState<InstalledMarketplacesState>(fallbackInstalledMarketplacesState);
  const [globalAgentsState, setGlobalAgentsState] =
    useState<GlobalAgentsState>(fallbackGlobalAgentsState);
  const [globalSkillsState, setGlobalSkillsState] =
    useState<GlobalSkillsState>(fallbackGlobalSkillsState);
  const [globalAssetStatus, setGlobalAssetStatus] = useState('');
  const [globalAssetActionId, setGlobalAssetActionId] = useState<string | null>(null);
  const [currentView, setCurrentView] = useState<'home' | 'profiles' | 'about'>('home');
  const [isTauriRuntime, setIsTauriRuntime] = useState(false);
  const [defaultProjectDisplayName, setDefaultProjectDisplayName] =
    useState(fallbackProjectDefaults.displayName);
  const [editingProfileId, setEditingProfileId] = useState<string | null>(null);
  const [profileForm, setProfileForm] = useState<ProfileFormState>(() =>
    createProfileForm(fallbackProjectDefaults.displayName, fallbackSummary.catalogRevision),
  );
  const [assetProfile, setAssetProfile] = useState<Profile | null>(null);
  const [profileAssets, setProfileAssets] = useState<ProfileAsset[]>([]);
  const [assetFilter, setAssetFilter] = useState('');
  const [assetModalStatus, setAssetModalStatus] = useState('');
  const [assetModalLoading, setAssetModalLoading] = useState(false);
  const [assetActionId, setAssetActionId] = useState<string | null>(null);

  async function loadMarketplaces() {
    try {
      const items = await invoke<MarketplaceOption[]>('list_marketplaces');
      setMarketplaces(items);
    } catch {
      setMarketplaces([]);
    }
  }

  async function loadProfiles() {
    try {
      const items = await invoke<Profile[]>('list_profiles');
      setProfiles(items);
    } catch {
      setProfiles([]);
    }
  }

  async function loadGlobalAgents() {
    try {
      const state = await invoke<GlobalAgentsState>('get_global_agents_state');
      setGlobalAgentsState(state);
    } catch {
      setGlobalAgentsState(fallbackGlobalAgentsState);
    }
  }

  async function loadGlobalSkills() {
    try {
      const state = await invoke<GlobalSkillsState>('get_global_skills_state');
      setGlobalSkillsState(state);
    } catch {
      setGlobalSkillsState(fallbackGlobalSkillsState);
    }
  }

  async function loadInstalledMarketplaces() {
    try {
      const state = await invoke<InstalledMarketplacesState>('get_installed_marketplaces_state');
      setInstalledMarketplacesState(state);
    } catch {
      setInstalledMarketplacesState(fallbackInstalledMarketplacesState);
    }
  }

  async function loadProjectProfileDefaults(catalogRevision: string | null) {
    try {
      const defaults = await invoke<ProjectProfileDefaults>('get_project_profile_defaults');
      setDefaultProjectDisplayName(defaults.displayName);
      setProfileForm(createProfileForm(defaults.displayName, catalogRevision));
    } catch {
      setDefaultProjectDisplayName(fallbackProjectDefaults.displayName);
      setProfileForm(createProfileForm(fallbackProjectDefaults.displayName, catalogRevision));
    }
  }

  function currentCatalogRevision() {
    return details?.summary.catalogRevision ?? summary.catalogRevision;
  }

  function filterMarketplaceAgents(marketplace: MarketplaceOption) {
    const filterText = marketplaceAgentFilters[marketplace.id]?.trim().toLocaleLowerCase() ?? '';
    if (filterText.length === 0) {
      return marketplace.agents;
    }

    return marketplace.agents.filter((agent) =>
      agent.name.toLocaleLowerCase().includes(filterText),
    );
  }

  function filterMarketplacePlugins(marketplace: MarketplaceOption) {
    const filterText = marketplaceSkillFilters[marketplace.id]?.trim().toLocaleLowerCase() ?? '';
    if (filterText.length === 0) {
      return marketplace.plugins;
    }

    return marketplace.plugins
      .map((plugin) => {
        const pluginMatches = plugin.name.toLocaleLowerCase().includes(filterText);
        const skills = pluginMatches
          ? plugin.skills
          : plugin.skills.filter((skill) => skill.name.toLocaleLowerCase().includes(filterText));
        return { ...plugin, skills };
      })
      .filter((plugin) => plugin.skills.length > 0 || plugin.name.toLocaleLowerCase().includes(filterText));
  }

  function startNewProfile() {
    setEditingProfileId(null);
    setProfileForm(createProfileForm(defaultProjectDisplayName, currentCatalogRevision()));
    setStatus('Ready to create a new project.');
  }

  function editProfile(profile: Profile) {
    setEditingProfileId(profile.id);
    setProfileForm(profileToForm(profile));
    setStatus(`Editing project ${profile.id}.`);
  }

  function updateProjectLocation(projectLocation: string) {
    setProfileForm((current) => {
      const guessedName = guessProjectName(projectLocation);
      const previousGuess = guessProjectName(current.project_location);
      const shouldReplaceName =
        current.name.trim().length === 0 ||
        current.name === previousGuess ||
        (current.project_location.trim().length === 0 &&
          current.name === defaultProjectDisplayName);

      return {
        ...current,
        project_location: projectLocation,
        name: guessedName && shouldReplaceName ? guessedName : current.name,
      };
    });
  }

  async function chooseProjectLocation() {
    if (!isTauriRuntime) {
      setStatus('Folder selection is available only in the Tauri desktop app.');
      return;
    }

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: normalizeOptionalText(profileForm.project_location) ?? undefined,
      });

      if (typeof selected === 'string') {
        updateProjectLocation(selected);
        setStatus(`Selected project folder ${selected}.`);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`Could not open the folder picker: ${message}`);
    }
  }

  useEffect(() => {
    if (startupLoadStarted.current) {
      return;
    }
    startupLoadStarted.current = true;

    const load = async () => {
      let runningInsideTauri = true;
      let catalogRevision = fallbackSummary.catalogRevision;

      try {
        const state = await invoke<CatalogState>('get_catalog_state');
        setIsTauriRuntime(true);
        setSummary(state.summary);
        setDetails(state);
        catalogRevision = state.summary.catalogRevision;
        setStatus(
          state.stale ? 'Showing the bundled trusted catalog.' : 'Trusted marketplaces are ready.',
        );
      } catch {
        runningInsideTauri = false;
        setIsTauriRuntime(false);
        setSummary(fallbackSummary);
        setDetails(fallbackDetails);
        setMarketplaces(fallbackMarketplaces);
        setProfiles([]);
        setStatus('Showing the bundled trusted catalog.');
      }

      if (runningInsideTauri) {
        try {
          await invoke<void>('refresh_startup_data');
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          setStatus(`Some startup data could not be refreshed: ${message}`);
        }

        await Promise.all([
          loadMarketplaces(),
          loadProfiles(),
          loadInstalledMarketplaces(),
          loadGlobalAgents(),
          loadGlobalSkills(),
          loadProjectProfileDefaults(catalogRevision),
        ]);
      } else {
        setDefaultProjectDisplayName(fallbackProjectDefaults.displayName);
        setProfileForm(createProfileForm(fallbackProjectDefaults.displayName, catalogRevision));
        setInstalledMarketplacesState(fallbackInstalledMarketplacesState);
        setGlobalAgentsState(fallbackGlobalAgentsState);
        setGlobalSkillsState(fallbackGlobalSkillsState);
      }

      setIsLoading(false);
    };

    void load();
  }, []);

  async function saveProfile() {
    const projectLocation = normalizeOptionalText(profileForm.project_location);
    if (projectLocation === null) {
      setStatus('Select the project repository root before saving.');
      return;
    }

    const profile: Profile = {
      id: profileForm.id,
      name: profileForm.name.trim(),
      project_location: projectLocation,
      description: normalizeOptionalText(profileForm.description),
      enabled: profileForm.enabled,
      version: profileForm.version,
      catalogRevision: profileForm.catalogRevision ?? currentCatalogRevision(),
      selected_assets: profileForm.selected_assets,
    };

    const savedProfile = await invoke<Profile>('upsert_profile', { profile });
    await loadProfiles();
    setEditingProfileId(savedProfile.id);
    setProfileForm(profileToForm(savedProfile));
    setStatus(`Saved project ${savedProfile.id}.`);
  }

  async function removeProfile(id: string) {
    await invoke<void>('delete_profile', { profileId: id });
    await loadProfiles();
    if (editingProfileId === id) {
      startNewProfile();
    }
    setStatus(`Deleted project ${id}.`);
  }

  async function loadProfileAssets(profileId: string): Promise<boolean> {
    setAssetModalLoading(true);
    try {
      const assets = await invoke<ProfileAsset[]>('list_profile_assets', { profileId });
      setProfileAssets(assets);
      setAssetModalStatus('');
      return true;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setProfileAssets([]);
      setAssetModalStatus(`Could not load trusted assets: ${message}`);
      return false;
    } finally {
      setAssetModalLoading(false);
    }
  }

  async function refreshTrustedCatalog() {
    if (assetProfile === null || assetModalLoading) {
      return;
    }
    setAssetModalLoading(true);
    setAssetModalStatus('Refreshing the trusted catalog from its main branch…');
    try {
      await invoke('refresh_catalog');
      const state = await invoke<CatalogState>('get_catalog_state');
      setSummary(state.summary);
      setDetails(state);
      await loadMarketplaces();
      if (await loadProfileAssets(assetProfile.id)) {
        setAssetModalStatus('Trusted catalog refreshed. Assets are ready.');
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setAssetModalStatus(`Could not refresh the trusted catalog: ${message}`);
    } finally {
      setAssetModalLoading(false);
    }
  }

  function openProfileAssets(profile: Profile) {
    if (!isTauriRuntime) {
      setStatus('Asset installation is available only in the Tauri desktop app.');
      return;
    }
    setAssetProfile(profile);
    setAssetFilter('');
    setAssetModalStatus('');
    void loadProfileAssets(profile.id);
  }

  async function toggleProfileAsset(asset: ProfileAsset) {
    if (assetProfile === null || assetActionId !== null) {
      return;
    }
    setAssetActionId(asset.asset_id);
    setAssetModalStatus('');
    try {
      const command =
        asset.installation_state === 'Installed'
          ? 'uninstall_profile_asset'
          : 'install_profile_asset';
      await invoke<void>(command, { profileId: assetProfile.id, assetId: asset.asset_id });
      await loadProfileAssets(assetProfile.id);
      setAssetModalStatus(
        `${asset.installation_state === 'Installed' ? 'Uninstalled' : 'Installed'} ${asset.name}.`,
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setAssetModalStatus(`Could not update ${asset.name}: ${message}`);
    } finally {
      setAssetActionId(null);
    }
  }

  async function removeGlobalAsset(asset: GlobalInventoryAsset) {
    if (globalAssetActionId !== null || !isTauriRuntime) {
      return;
    }
    const action = asset.trust_status === 'Trusted' ? 'Uninstall' : 'Delete';
    const confirmed = window.confirm(
      `${action} ${asset.kind} "${asset.name}" from ${asset.location}? This removes only the selected ${asset.kind}.`,
    );
    if (!confirmed) {
      return;
    }

    setGlobalAssetActionId(asset.key);
    setGlobalAssetStatus('');
    try {
      await invoke<void>('remove_local_asset', {
        kind: asset.kind,
        source: asset.source,
        location: asset.location,
      });
      try {
        await invoke<void>('refresh_global_inventory');
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        const pastAction = action === 'Uninstall' ? 'Uninstalled' : 'Deleted';
        setGlobalAssetStatus(
          `${pastAction} ${asset.name}, but the inventory refresh failed: ${message}`,
        );
        await Promise.all([
          loadInstalledMarketplaces(),
          loadGlobalAgents(),
          loadGlobalSkills(),
        ]);
        return;
      }
      await Promise.all([
        loadInstalledMarketplaces(),
        loadGlobalAgents(),
        loadGlobalSkills(),
      ]);
      setGlobalAssetStatus(`${action === 'Uninstall' ? 'Uninstalled' : 'Deleted'} ${asset.name}.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setGlobalAssetStatus(`Could not ${action.toLowerCase()} ${asset.name}: ${message}`);
    } finally {
      setGlobalAssetActionId(null);
    }
  }

  const globalInventory = buildGlobalInventory(
    globalAgentsState,
    globalSkillsState,
    installedMarketplacesState,
  );

  if (isLoading) {
    return (
      <main className="startup-screen" aria-busy="true">
        <section className="startup-content" role="status" aria-live="polite">
          <h1>Tupi</h1>
          <p>Preparing your workspace</p>
          <div className="startup-progress" role="progressbar" aria-label="Loading Tupi">
            <span />
          </div>
        </section>
      </main>
    );
  }

  return (
    <main className="shell">
      <section className="hero">
        <h1>Tupi</h1>
        <p className="lede">
          Desktop trust authority for catalog-approved AI assets and project profiles.
        </p>
        <nav className="view-switcher" aria-label="Application sections">
          <button
            type="button"
            className={currentView === 'home' ? 'tab-button active' : 'tab-button'}
            onClick={() => setCurrentView('home')}
          >
            Home
          </button>
          <button
            type="button"
            className={currentView === 'profiles' ? 'tab-button active' : 'tab-button'}
            onClick={() => setCurrentView('profiles')}
          >
            Profiles
          </button>
          <button
            type="button"
            className={currentView === 'about' ? 'tab-button active' : 'tab-button'}
            onClick={() => setCurrentView('about')}
          >
            About
          </button>
        </nav>
      </section>

      {currentView === 'home' ? (
        <>
          <section className="panel">
            <h2>Home</h2>
            <p className="lede">
              Manage agents and skills installed for your user. Trusted assets are uninstalled;
              untrusted assets are deleted.
            </p>
            {!isTauriRuntime ? (
              <p className="status">
                Local asset discovery and removal are available only in the Tauri desktop app.
              </p>
            ) : (
              <>
                <div className="home-section">
                  <h3>Global</h3>
                  <dl className="grid compact-grid">
                    <div>
                      <dt>Global agents</dt>
                      <dd>{globalAgentsState.scan_root}</dd>
                    </div>
                    <div>
                      <dt>Global skills</dt>
                      <dd>{globalSkillsState.scan_root}</dd>
                    </div>
                    <div>
                      <dt>Installed marketplace plugins</dt>
                      <dd>{installedMarketplacesState.scan_root}</dd>
                    </div>
                  </dl>
                  <div className="actions">
                    <button
                      type="button"
                      className="secondary"
                      disabled={globalAssetActionId !== null}
                      onClick={() => {
                        void (async () => {
                          setGlobalAssetStatus('Refreshing local inventory…');
                          try {
                            await invoke<void>('refresh_global_inventory');
                            await Promise.all([
                              loadInstalledMarketplaces(),
                              loadGlobalAgents(),
                              loadGlobalSkills(),
                            ]);
                            setGlobalAssetStatus('Local inventory refreshed.');
                          } catch (error) {
                            const message = error instanceof Error ? error.message : String(error);
                            setGlobalAssetStatus(`Could not refresh local inventory: ${message}`);
                          }
                        })();
                      }}
                    >
                      Refresh inventory
                    </button>
                  </div>
                  {globalAssetStatus ? (
                    <p className="status" role="status">{globalAssetStatus}</p>
                  ) : null}
                  {globalAgentsState.error_message ? (
                    <p className="status" role="alert">
                      Global agent scan failed: {globalAgentsState.error_message}
                    </p>
                  ) : null}
                  {globalSkillsState.error_message ? (
                    <p className="status" role="alert">
                      Global skill scan failed: {globalSkillsState.error_message}
                    </p>
                  ) : null}
                  {installedMarketplacesState.error_message ? (
                    <p className="status" role="alert">
                      Installed marketplace scan failed:{' '}
                      {installedMarketplacesState.error_message}
                    </p>
                  ) : null}
                </div>
                <div className="home-section">
                  <h4>Agents</h4>
                  <div className="global-asset-list">
                    {globalInventory.filter((asset) => asset.kind === 'agent').length === 0 ? (
                      <p className="status">No user-level agents were found.</p>
                    ) : (
                      globalInventory
                        .filter((asset) => asset.kind === 'agent')
                        .map((asset) => (
                          <article className="global-asset-card" key={asset.key}>
                            <div className="global-asset-details">
                              <header>
                                <strong>{asset.name}</strong>
                                <span
                                  className={`trust-badge ${
                                    asset.trust_status === 'Trusted' ? 'trusted' : 'untrusted'
                                  }`}
                                >
                                  {asset.trust_status}
                                </span>
                              </header>
                              {asset.parent ? <p className="asset-source">{asset.parent}</p> : null}
                              {asset.description ? <p>{asset.description}</p> : null}
                              <p className="agent-path">{asset.location}</p>
                            </div>
                            <button
                              type="button"
                              className={asset.trust_status === 'Trusted' ? 'secondary' : 'danger'}
                              disabled={globalAssetActionId !== null}
                              onClick={() => void removeGlobalAsset(asset)}
                            >
                              {globalAssetActionId === asset.key
                                ? 'Working…'
                                : asset.trust_status === 'Trusted'
                                  ? 'Uninstall'
                                  : 'Delete'}
                            </button>
                          </article>
                        ))
                    )}
                  </div>
                </div>
                <div className="home-section">
                  <h4>Skills</h4>
                  <div className="global-asset-list">
                    {globalInventory.filter((asset) => asset.kind === 'skill').length === 0 ? (
                      <p className="status">No user-level skills were found.</p>
                    ) : (
                      globalInventory
                        .filter((asset) => asset.kind === 'skill')
                        .map((asset) => (
                          <article className="global-asset-card" key={asset.key}>
                            <div className="global-asset-details">
                              <header>
                                <strong>{asset.name}</strong>
                                <span
                                  className={`trust-badge ${
                                    asset.trust_status === 'Trusted' ? 'trusted' : 'untrusted'
                                  }`}
                                >
                                  {asset.trust_status}
                                </span>
                              </header>
                              {asset.parent ? <p className="asset-source">{asset.parent}</p> : null}
                              <p className="agent-path">{asset.location}</p>
                            </div>
                            <button
                              type="button"
                              className={asset.trust_status === 'Trusted' ? 'secondary' : 'danger'}
                              disabled={globalAssetActionId !== null}
                              onClick={() => void removeGlobalAsset(asset)}
                            >
                              {globalAssetActionId === asset.key
                                ? 'Working…'
                                : asset.trust_status === 'Trusted'
                                  ? 'Uninstall'
                                  : 'Delete'}
                            </button>
                          </article>
                        ))
                    )}
                  </div>
                </div>
              </>
            )}
          </section>
        </>
      ) : currentView === 'profiles' ? (
        <>
          <section className="panel">
            <h2>Project profiles</h2>
            <p className="lede">
              Each project keeps a database-backed ID, the repository root location, a guessed
              display name, and an optional description.
            </p>
            <div className="form-grid">
              <label>
                Project ID
                <input
                  readOnly
                  value={profileForm.id}
                  placeholder="Generated when saved"
                />
                <p className="field-hint">Generated by the embedded database when you save.</p>
              </label>
              <label>
                Project location
                <div className="input-with-action">
                  <input
                    value={profileForm.project_location}
                    placeholder="Choose the repository root folder"
                    onChange={(event) => updateProjectLocation(event.target.value)}
                  />
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => void chooseProjectLocation()}
                    disabled={!isTauriRuntime}
                    title={
                      isTauriRuntime
                        ? 'Open the system folder picker'
                        : 'Folder selection requires the Tauri desktop runtime'
                    }
                  >
                    Select folder
                  </button>
                </div>
                <p className="field-hint">
                  {isTauriRuntime
                    ? 'Save the repository root here. Tupi uses the folder name to guess the project name.'
                    : 'Folder selection is only available in the Tauri desktop runtime. You can still paste the repository root manually.'}
                </p>
              </label>
              <label>
                Display name
                <input
                  value={profileForm.name}
                  onChange={(event) => setProfileForm({ ...profileForm, name: event.target.value })}
                />
              </label>
              <label className="form-span-full">
                Description
                <textarea
                  rows={4}
                  value={profileForm.description}
                  placeholder="Describe the project"
                  onChange={(event) =>
                    setProfileForm({ ...profileForm, description: event.target.value })
                  }
                />
              </label>
            </div>
            <div className="actions">
              <button type="button" onClick={() => void saveProfile()}>
                {editingProfileId === null ? 'Create project' : 'Save project'}
              </button>
              <button type="button" className="secondary" onClick={startNewProfile}>
                {editingProfileId === null ? 'Reset form' : 'New project'}
              </button>
            </div>
            <p className="status">{status}</p>
            <div className="profile-list">
              {profiles.length === 0 ? (
                <p className="status">No project profiles saved yet.</p>
              ) : (
                profiles.map((profile) => (
                  <article key={profile.id} className="profile-card">
                    <header>
                      <strong>{profile.name}</strong>
                      <code>{profile.id}</code>
                    </header>
                    <p>{profile.project_location ?? 'No project location saved.'}</p>
                    <p>{profile.description ?? 'No description yet.'}</p>
                    <div className="actions">
                      <button
                        type="button"
                        onClick={() => openProfileAssets(profile)}
                        disabled={!isTauriRuntime || !profile.project_location}
                        title={
                          !profile.project_location
                            ? 'Set a project folder before installing assets'
                            : 'Install or uninstall trusted agents and skills for this project'
                        }
                      >
                        Agents &amp; skills
                      </button>
                      <button type="button" className="secondary" onClick={() => editProfile(profile)}>
                        Edit
                      </button>
                      <button type="button" onClick={() => void removeProfile(profile.id)}>
                        Delete
                      </button>
                    </div>
                  </article>
                ))
              )}
            </div>
          </section>
        </>
      ) : (
        <>
          <section className="panel">
            <h2>About Tupi</h2>
            <p className="lede">
              Tupi ships a curated catalog of trusted marketplaces and keeps trust enforcement in
              the native core.
            </p>
            <dl className="grid">
              <div>
                <dt>Catalog version</dt>
                <dd>{summary.version}</dd>
              </div>
              <div>
                <dt>Catalog revision</dt>
                <dd>{summary.catalogRevision}</dd>
              </div>
            </dl>
            {details?.stale ? <p className="status">Showing the bundled catalog metadata.</p> : null}
          </section>
          <section className="panel">
            <h2>Trusted marketplaces</h2>
            {marketplaces.length === 0 ? (
              <p className="status">No trusted marketplaces are available yet.</p>
            ) : (
              <div className="marketplace-list">
                {marketplaces.map((marketplace) => {
                  const filteredAgents = filterMarketplaceAgents(marketplace);
                  const filteredPlugins = filterMarketplacePlugins(marketplace);
                  const filterText =
                    marketplaceSkillFilters[marketplace.id]?.trim().toLocaleLowerCase() ?? '';
                  const filteredMarketplaceSkills =
                    filterText.length === 0
                      ? marketplace.skills
                      : marketplace.skills.filter((skill) =>
                          skill.name.toLocaleLowerCase().includes(filterText),
                        );
                  const skillCount =
                    marketplace.skills.length +
                    marketplace.plugins.reduce((count, plugin) => count + plugin.skills.length, 0);

                  return (
                    <article key={marketplace.id} className="marketplace-card">
                      <header>
                        <strong>{marketplace.name}</strong>
                        <code>{marketplace.id}</code>
                      </header>
                      <p className="marketplace-repository">{marketplace.repository}</p>
                      <details className="marketplace-agents">
                        <summary>
                          <span>Agents</span>
                          <span className="marketplace-agent-count">
                            {marketplace.agents.length === 0
                              ? 'No trusted agents cached'
                              : `${marketplace.agents.length} trusted agent${
                                  marketplace.agents.length === 1 ? '' : 's'
                                }`}
                          </span>
                        </summary>
                        {marketplace.agents.length === 0 ? (
                          <p className="marketplace-agent-empty">
                            No trusted marketplace agents are cached yet.
                          </p>
                        ) : (
                          <>
                            <label className="marketplace-agent-filter">
                              Filter agents by name
                              <input
                                value={marketplaceAgentFilters[marketplace.id] ?? ''}
                                placeholder="Type part of an agent name"
                                onChange={(event) =>
                                  setMarketplaceAgentFilters((current) => ({
                                    ...current,
                                    [marketplace.id]: event.target.value,
                                  }))
                                }
                              />
                            </label>
                            {filteredAgents.length === 0 ? (
                              <p className="marketplace-agent-empty">
                                No agents match the current filter.
                              </p>
                            ) : (
                              <ul className="marketplace-agent-items">
                                {filteredAgents.map((agent) => (
                                  <li
                                    key={`${marketplace.id}-${agent.name}`}
                                    className="marketplace-agent-item"
                                  >
                                    <strong>{agent.name}</strong>
                                    <p>{agent.description ?? 'No frontmatter description provided.'}</p>
                                  </li>
                                ))}
                              </ul>
                            )}
                          </>
                        )}
                      </details>
                      <details className="marketplace-skills">
                        <summary>
                          <span>Skills</span>
                          <span className="marketplace-agent-count">
                            {skillCount === 0
                              ? 'No available skills found'
                              : `${skillCount} available skill${skillCount === 1 ? '' : 's'}`}
                          </span>
                        </summary>
                        {skillCount === 0 ? (
                          <p className="marketplace-agent-empty">
                            No skill folders were found in this trusted marketplace yet.
                          </p>
                        ) : (
                          <>
                            <label className="marketplace-agent-filter">
                              Filter skills by name or plugin
                              <input
                                value={marketplaceSkillFilters[marketplace.id] ?? ''}
                                placeholder="Type part of a skill or plugin name"
                                onChange={(event) =>
                                  setMarketplaceSkillFilters((current) => ({
                                    ...current,
                                    [marketplace.id]: event.target.value,
                                  }))
                                }
                              />
                            </label>
                            {filteredPlugins.length === 0 && filteredMarketplaceSkills.length === 0 ? (
                              <p className="marketplace-agent-empty">
                                No skills match the current filter.
                              </p>
                            ) : (
                              <>
                                {filteredMarketplaceSkills.length > 0 ? (
                                  <section aria-label={`Marketplace skills in ${marketplace.name}`}>
                                    <div className="marketplace-plugin-heading">
                                      <strong>Marketplace skills</strong>
                                      <span className="marketplace-plugin-count">
                                        {filteredMarketplaceSkills.length} skill
                                        {filteredMarketplaceSkills.length === 1 ? '' : 's'}
                                      </span>
                                    </div>
                                    <ul className="marketplace-skill-items">
                                      {filteredMarketplaceSkills.map((skill) => (
                                        <li
                                          key={`${marketplace.id}-${skill.name}`}
                                          className="marketplace-plugin-item"
                                        >
                                          <strong>{skill.name}</strong>
                                          <p className="agent-path">{skill.directory_location}</p>
                                        </li>
                                      ))}
                                    </ul>
                                  </section>
                                ) : null}
                                {filteredPlugins.length > 0 ? (
                                  <ul className="marketplace-plugin-items">
                                    {filteredPlugins.map((plugin) => (
                                      <li
                                        key={`${marketplace.id}-${plugin.name}`}
                                        className="marketplace-skill-plugin"
                                      >
                                        <div className="marketplace-plugin-heading">
                                          <strong>{plugin.name}</strong>
                                          <span className="marketplace-plugin-count">
                                            {plugin.skills.length} skill
                                            {plugin.skills.length === 1 ? '' : 's'}
                                          </span>
                                        </div>
                                        {plugin.skills.length === 0 ? (
                                          <p className="marketplace-plugin-empty">
                                            No skills available in this plugin.
                                          </p>
                                        ) : (
                                          <ul className="marketplace-skill-items">
                                            {plugin.skills.map((skill) => (
                                              <li
                                                key={`${plugin.name}-${skill.name}`}
                                                className="marketplace-plugin-item"
                                              >
                                                <strong>{skill.name}</strong>
                                                <p className="agent-path">{skill.directory_location}</p>
                                              </li>
                                            ))}
                                          </ul>
                                        )}
                                      </li>
                                    ))}
                                  </ul>
                                ) : null}
                              </>
                            )}
                          </>
                        )}
                      </details>
                    </article>
                  );
                })}
              </div>
            )}
          </section>
        </>
      )}
      {assetProfile !== null ? (
        <div className="asset-modal-backdrop" role="presentation">
          <section
            className="asset-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="asset-modal-title"
          >
            <header className="asset-modal-header">
              <div>
                <h2 id="asset-modal-title">Agents &amp; skills for {assetProfile.name}</h2>
                <p className="agent-path">{assetProfile.project_location}</p>
              </div>
              <button
                type="button"
                className="secondary"
                onClick={() => setAssetProfile(null)}
                aria-label="Close asset manager"
              >
                Close
              </button>
            </header>
            <p className="lede">
              Trusted agents are copied to <code>.github/agents</code> and skills to{' '}
              <code>.github/skills</code>. Existing files are never overwritten; modified installed
              assets are preserved when uninstalling.
            </p>
            {details?.stale || details?.trust_status !== 'Trusted' ? (
              <p className="field-hint">
                The bundled catalog is a local scaffold until refreshed from the trusted Tupi
                repository. Refresh it to enable asset installation.
              </p>
            ) : null}
            <div className="actions">
              <button
                type="button"
                className="secondary"
                disabled={assetModalLoading || assetActionId !== null}
                onClick={() => void refreshTrustedCatalog()}
              >
                {assetModalLoading ? 'Refreshing…' : 'Refresh trusted catalog'}
              </button>
            </div>
            <label className="asset-filter">
              Filter agents and skills
              <input
                value={assetFilter}
                placeholder="Search by asset, marketplace, or plugin"
                onChange={(event) => setAssetFilter(event.target.value)}
              />
            </label>
            {assetModalLoading ? <p className="status">Loading trusted assets…</p> : null}
            {assetModalStatus ? <p className="status" role="status">{assetModalStatus}</p> : null}
            {!assetModalLoading && profileAssets.length === 0 && !assetModalStatus ? (
              <p className="status">No trusted agents or skills are available in the active catalog.</p>
            ) : null}
            <div className="profile-asset-list">
              {profileAssets
                .filter((asset) => {
                  const query = assetFilter.trim().toLocaleLowerCase();
                  return (
                    query.length === 0 ||
                    [asset.name, asset.marketplace_name, asset.plugin_name ?? '', asset.kind]
                      .some((value) => value.toLocaleLowerCase().includes(query))
                  );
                })
                .map((asset) => (
                  <article className="profile-asset-card" key={asset.asset_id}>
                    <div>
                      <header>
                        <strong>{asset.name}</strong>
                        <span className={`asset-state ${asset.installation_state.toLowerCase()}`}>
                          {asset.installation_state}
                        </span>
                      </header>
                      <p className="asset-source">
                        {asset.kind} · {asset.marketplace_name}
                        {asset.plugin_name ? ` · ${asset.plugin_name}` : ''}
                      </p>
                      {!asset.source_available ? (
                        <p className="field-hint">
                          This asset is no longer in the active trusted catalog.
                        </p>
                      ) : null}
                      {asset.description ? <p>{asset.description}</p> : null}
                      <p className="agent-path">{asset.destination}</p>
                      {asset.installation_state === 'Conflict' ? (
                        <p className="field-hint">A file already exists here and is not managed by Tupi.</p>
                      ) : null}
                      {asset.installation_state === 'Modified' ? (
                        <p className="field-hint">This Tupi-installed asset was edited; it will not be removed.</p>
                      ) : null}
                    </div>
                    {asset.installation_state === 'Available' ||
                    asset.installation_state === 'Installed' ? (
                      <button
                        type="button"
                        className={asset.installation_state === 'Installed' ? 'secondary' : ''}
                        disabled={assetActionId !== null}
                        onClick={() => void toggleProfileAsset(asset)}
                      >
                        {assetActionId === asset.asset_id
                          ? 'Working…'
                          : asset.installation_state === 'Installed'
                            ? 'Uninstall'
                            : 'Install'}
                      </button>
                    ) : null}
                  </article>
                ))}
            </div>
          </section>
        </div>
      ) : null}
    </main>
  );
}
