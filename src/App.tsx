import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/api/dialog';
import { invoke } from '@tauri-apps/api/tauri';

type CatalogSummary = {
  version: number;
  catalogRevision: string;
  marketplaces: number;
  assets: number;
};

type CatalogState = {
  summary: CatalogSummary;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
  source_repository: string | null;
  source_branch: string;
  refreshed_at: string | null;
  stale: boolean;
};

type MarketplaceOption = {
  id: string;
  name: string;
  repository: string;
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
  },
];

const fallbackProjectDefaults: ProjectProfileDefaults = {
  displayName: 'Project',
};

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
  const [summary, setSummary] = useState<CatalogSummary>(fallbackSummary);
  const [status, setStatus] = useState('Trusted marketplaces are ready.');
  const [details, setDetails] = useState<CatalogState | null>(null);
  const [marketplaces, setMarketplaces] = useState<MarketplaceOption[]>([]);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [currentView, setCurrentView] = useState<'home' | 'about'>('home');
  const [isTauriRuntime, setIsTauriRuntime] = useState(false);
  const [defaultProjectDisplayName, setDefaultProjectDisplayName] =
    useState(fallbackProjectDefaults.displayName);
  const [editingProfileId, setEditingProfileId] = useState<string | null>(null);
  const [profileForm, setProfileForm] = useState<ProfileFormState>(() =>
    createProfileForm(fallbackProjectDefaults.displayName, fallbackSummary.catalogRevision),
  );

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
        await Promise.all([
          loadMarketplaces(),
          loadProfiles(),
          loadProjectProfileDefaults(catalogRevision),
        ]);
      } else {
        setDefaultProjectDisplayName(fallbackProjectDefaults.displayName);
        setProfileForm(createProfileForm(fallbackProjectDefaults.displayName, catalogRevision));
      }
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
            <h2>Trusted marketplaces</h2>
            {marketplaces.length === 0 ? (
              <p className="status">No trusted marketplaces are available yet.</p>
            ) : (
              <div className="marketplace-list">
                {marketplaces.map((marketplace) => (
                  <article key={marketplace.id} className="marketplace-card">
                    <header>
                      <strong>{marketplace.name}</strong>
                      <code>{marketplace.id}</code>
                    </header>
                    <p className="marketplace-repository">{marketplace.repository}</p>
                  </article>
                ))}
              </div>
            )}
          </section>
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
      )}
    </main>
  );
}
