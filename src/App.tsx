import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/tauri';

type CatalogSummary = {
  version: number;
  catalogRevision: string;
  marketplaces: number;
  assets: number;
};

type RefreshRecord = {
  summary: CatalogSummary;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
  source_repository: string | null;
  source_branch: string;
  refreshed_at: string;
};

type CatalogState = {
  summary: CatalogSummary;
  trust_status: 'Trusted' | 'Untrusted' | 'Stale' | 'Invalid' | 'Missing';
  source_repository: string | null;
  source_branch: string;
  refreshed_at: string | null;
  stale: boolean;
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
  catalogRevision: 'uninitialized',
  marketplaces: 0,
  assets: 0,
};

export default function App() {
  const [summary, setSummary] = useState<CatalogSummary>(fallbackSummary);
  const [status, setStatus] = useState('Ready to refresh the trusted catalog.');
  const [details, setDetails] = useState<CatalogState | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [profileForm, setProfileForm] = useState({
    id: 'project-a',
    name: 'Project A',
    enabled: true,
    assets: 'code-reviewer',
  });
  const [refreshing, setRefreshing] = useState(false);

  const trustIndicators = useMemo(
    () => [
      'Trust decisions live in the Rust core.',
      'Only catalog-listed assets can be activated.',
      'Refreshes fail closed and preserve the last valid cache.',
    ],
    [],
  );

  useEffect(() => {
    const load = async () => {
      try {
        const state = await invoke<CatalogState>('get_catalog_state');
        setSummary(state.summary);
        setDetails(state);
        setStatus(
          state.stale
            ? 'Loaded stale trusted catalog cache.'
            : `Loaded trusted catalog cache from ${state.refreshed_at ?? 'startup'}.`,
        );
      } catch {
        setStatus('Running outside Tauri; showing local scaffold state.');
      }

      try {
        const items = await invoke<Profile[]>('list_profiles');
        setProfiles(items);
      } catch {
        setProfiles([]);
      }
    };

    void load();
  }, []);

  async function refreshCatalog() {
    setRefreshing(true);
    setStatus('Refreshing trusted catalog...');
    try {
      const record = await invoke<RefreshRecord>('refresh_catalog');
      setSummary(record.summary);
      setDetails({
        summary: record.summary,
        trust_status: record.trust_status,
        source_repository: record.source_repository,
        source_branch: record.source_branch,
        refreshed_at: record.refreshed_at,
        stale: false,
      });
      setStatus(`Catalog refreshed as ${record.trust_status} at ${record.refreshed_at}.`);
    } catch (error) {
      setStatus(
        error instanceof Error ? error.message : 'Refresh failed before trust could be established.',
      );
    } finally {
      setRefreshing(false);
    }

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
      const items = await invoke<Profile[]>('list_profiles');
      setProfiles(items);
      setStatus(`Saved profile ${profile.id}.`);
    }

    async function removeProfile(id: string) {
      await invoke<void>('delete_profile', { profileId: id });
      const items = await invoke<Profile[]>('list_profiles');
      setProfiles(items);
      setStatus(`Deleted profile ${id}.`);
    }
  }

  return (
    <main className="shell">
      <section className="hero">
        <h1>Tupi</h1>
        <p className="lede">
          Desktop trust authority for catalog-approved AI assets and project profiles.
        </p>
      </section>

      <section className="panel">
        <h2>Catalog state</h2>
        <dl className="grid">
          <div>
            <dt>Version</dt>
            <dd>{summary.version}</dd>
          </div>
          <div>
            <dt>Catalog revision</dt>
            <dd>{summary.catalogRevision}</dd>
          </div>
          <div>
            <dt>Marketplaces</dt>
            <dd>{summary.marketplaces}</dd>
          </div>
          <div>
            <dt>Assets</dt>
            <dd>{summary.assets}</dd>
          </div>
          <div>
            <dt>Source branch</dt>
            <dd>{details?.source_branch ?? 'main'}</dd>
          </div>
          <div>
            <dt>Trust status</dt>
            <dd>{details?.trust_status ?? 'Trusted'}</dd>
          </div>
          <div>
            <dt>Refresh state</dt>
            <dd>{details?.stale ? 'Stale' : 'Current'}</dd>
          </div>
        </dl>

        <div className="actions">
          <button
            type="button"
            onClick={() => {
              void refreshCatalog();
            }}
            disabled={refreshing}
          >
            {refreshing ? 'Refreshing…' : 'Refresh trusted catalog'}
          </button>
        </div>

        <p className="status">{status}</p>
      </section>

      <section className="panel">
        <h2>MVP trust rules</h2>
        <ul>
          {trustIndicators.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
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
              onChange={(event) => setProfileForm({ ...profileForm, assets: event.target.value })}
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
    </main>
  );
}
