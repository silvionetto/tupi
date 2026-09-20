import { useEffect, useState } from 'react';
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

export default function App() {
  const [summary, setSummary] = useState<CatalogSummary>(fallbackSummary);
  const [status, setStatus] = useState('Trusted marketplaces are ready.');
  const [details, setDetails] = useState<CatalogState | null>(null);
  const [marketplaces, setMarketplaces] = useState<MarketplaceOption[]>([]);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [currentView, setCurrentView] = useState<'home' | 'about'>('home');
  const [profileForm, setProfileForm] = useState({
    id: 'project-a',
    name: 'Project A',
    enabled: true,
    assets: 'code-reviewer',
  });

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

  useEffect(() => {
    const load = async () => {
      let runningInsideTauri = true;

      try {
        const state = await invoke<CatalogState>('get_catalog_state');
        setSummary(state.summary);
        setDetails(state);
        setStatus(
          state.stale ? 'Showing the bundled trusted catalog.' : 'Trusted marketplaces are ready.',
        );
      } catch {
        runningInsideTauri = false;
        setSummary(fallbackSummary);
        setDetails(fallbackDetails);
        setMarketplaces(fallbackMarketplaces);
        setProfiles([]);
        setStatus('Showing the bundled trusted catalog.');
      }

      if (runningInsideTauri) {
        await Promise.all([loadMarketplaces(), loadProfiles()]);
      }
    };

    void load();
  }, []);

  async function saveProfile() {
    const profile: Profile = {
      id: profileForm.id.trim(),
      name: profileForm.name.trim(),
      enabled: profileForm.enabled,
      version: '1',
      catalogRevision: details?.summary.catalogRevision ?? summary.catalogRevision,
      selected_assets: profileForm.assets
        .split(',')
        .map((asset) => asset.trim())
        .filter(Boolean),
    };

    await invoke<void>('upsert_profile', { profile });
    await loadProfiles();
    setStatus(`Saved profile ${profile.id}.`);
  }

  async function removeProfile(id: string) {
    await invoke<void>('delete_profile', { profileId: id });
    await loadProfiles();
    setStatus(`Deleted profile ${id}.`);
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
            <div className="form-grid">
              <label>
                Profile ID
                <input
                  value={profileForm.id}
                  onChange={(event) => setProfileForm({ ...profileForm, id: event.target.value })}
                />
              </label>
              <label>
                Display name
                <input
                  value={profileForm.name}
                  onChange={(event) => setProfileForm({ ...profileForm, name: event.target.value })}
                />
              </label>
              <label>
                Approved asset IDs
                <input
                  value={profileForm.assets}
                  onChange={(event) =>
                    setProfileForm({ ...profileForm, assets: event.target.value })
                  }
                />
              </label>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={profileForm.enabled}
                  onChange={(event) =>
                    setProfileForm({ ...profileForm, enabled: event.target.checked })
                  }
                />
                Enabled
              </label>
            </div>
            <div className="actions">
              <button type="button" onClick={() => void saveProfile()}>
                Save profile
              </button>
            </div>
            <p className="status">{status}</p>
            <div className="profile-list">
              {profiles.length === 0 ? (
                <p className="status">No profiles saved yet.</p>
              ) : (
                profiles.map((profile) => (
                  <article key={profile.id} className="profile-card">
                    <header>
                      <strong>{profile.name}</strong>
                      <span>{profile.enabled ? 'Enabled' : 'Disabled'}</span>
                    </header>
                    <p>
                      <code>{profile.id}</code>
                    </p>
                    <p>Catalog revision: {profile.catalogRevision ?? 'unresolved'}</p>
                    <p>Assets: {profile.selected_assets.join(', ') || 'none'}</p>
                    <div className="actions">
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
