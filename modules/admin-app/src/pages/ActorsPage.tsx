import { useState } from "react";
import { useQuery, useMutation } from "@apollo/client";
import { ACTORS } from "@/graphql/queries";
import { CREATE_ACTOR, SUBMIT_ACTOR } from "@/graphql/mutations";

type Actor = {
  id: string;
  name: string;
  actorType: string;
  description: string | null;
  signalCount: number;
};

type AddMode = "manual" | "url";

export function ActorsPage() {
  const region = "twincities";

  const { data, loading, refetch } = useQuery(ACTORS, {
    variables: { region, limit: 100 },
  });
  const actors: Actor[] = data?.actors ?? [];

  // --- Form state ---
  const [showForm, setShowForm] = useState(false);
  const [mode, setMode] = useState<AddMode>("url");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // URL mode
  const [url, setUrl] = useState("");
  const [urlRegion, setUrlRegion] = useState("");

  // Manual mode
  const [name, setName] = useState("");
  const [actorType, setActorType] = useState("organization");
  const [location, setLocation] = useState("");
  const [bio, setBio] = useState("");
  const [socialUrls, setSocialUrls] = useState("");

  const [createActor] = useMutation(CREATE_ACTOR);
  const [submitActor] = useMutation(SUBMIT_ACTOR);

  const resetForm = () => {
    setUrl("");
    setUrlRegion("");
    setName("");
    setActorType("organization");
    setLocation("");
    setBio("");
    setSocialUrls("");
    setError(null);
    setSuccess(null);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    setSuccess(null);

    try {
      if (mode === "url") {
        const { data } = await submitActor({
          variables: {
            url: url.trim(),
            region: urlRegion.trim() || undefined,
          },
        });
        const loc = data?.submitActor?.locationName;
        setSuccess(`Actor created${loc ? ` in ${loc}` : ""}`);
      } else {
        const accounts = socialUrls
          .split("\n")
          .map((s) => s.trim())
          .filter(Boolean);
        const { data } = await createActor({
          variables: {
            name: name.trim(),
            actorType,
            location: location.trim(),
            bio: bio.trim() || undefined,
            socialAccounts: accounts.length > 0 ? accounts : undefined,
          },
        });
        const loc = data?.createActor?.locationName;
        setSuccess(`Actor "${name.trim()}" created${loc ? ` in ${loc}` : ""}`);
      }
      resetForm();
      refetch();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to create actor");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Actors</h1>
        <button
          onClick={() => {
            setShowForm(!showForm);
            if (showForm) resetForm();
          }}
          className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
        >
          {showForm ? "Cancel" : "Add Actor"}
        </button>
      </div>

      {/* Add actor form */}
      {showForm && (
        <form onSubmit={handleSubmit} className="space-y-3 max-w-lg">
          {/* Mode tabs */}
          <div className="flex gap-1 border-b border-border">
            {(["url", "manual"] as const).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => { setMode(m); setError(null); setSuccess(null); }}
                className={`px-3 py-2 text-sm -mb-px transition-colors ${
                  mode === m
                    ? "border-b-2 border-foreground text-foreground"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {m === "url" ? "From URL" : "Manual"}
              </button>
            ))}
          </div>

          {mode === "url" ? (
            <>
              <input
                type="url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="https://linktr.ee/org or org website"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                required
              />
              <input
                type="text"
                value={urlRegion}
                onChange={(e) => setUrlRegion(e.target.value)}
                placeholder="Fallback region (optional, e.g. Minneapolis, MN)"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
              />
            </>
          ) : (
            <>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Name"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                required
              />
              <select
                value={actorType}
                onChange={(e) => setActorType(e.target.value)}
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
              >
                <option value="organization">Organization</option>
                <option value="individual">Individual</option>
                <option value="government_body">Government Body</option>
                <option value="coalition">Coalition</option>
              </select>
              <input
                type="text"
                value={location}
                onChange={(e) => setLocation(e.target.value)}
                placeholder="Location (e.g. Minneapolis, MN)"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                required
              />
              <input
                type="text"
                value={bio}
                onChange={(e) => setBio(e.target.value)}
                placeholder="Bio (optional)"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
              />
              <textarea
                value={socialUrls}
                onChange={(e) => setSocialUrls(e.target.value)}
                placeholder="Social URLs (one per line, optional)"
                rows={3}
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
              />
            </>
          )}

          {error && <p className="text-sm text-red-400">{error}</p>}
          {success && <p className="text-sm text-green-400">{success}</p>}

          <button
            type="submit"
            disabled={submitting}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90 disabled:opacity-50"
          >
            {submitting
              ? mode === "url"
                ? "Extracting..."
                : "Creating..."
              : mode === "url"
                ? "Extract Actor"
                : "Create Actor"}
          </button>
        </form>
      )}

      {/* Actor list */}
      {loading ? (
        <p className="text-muted-foreground">Loading actors...</p>
      ) : actors.length === 0 ? (
        <p className="text-muted-foreground">No actors found.</p>
      ) : (
        <div className="rounded-lg border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/50">
                <th className="text-left px-4 py-2 font-medium">Name</th>
                <th className="text-left px-4 py-2 font-medium">Type</th>
                <th className="text-right px-4 py-2 font-medium">Signals</th>
              </tr>
            </thead>
            <tbody>
              {actors.map((a) => (
                <tr key={a.id} className="border-b border-border last:border-0 hover:bg-muted/30">
                  <td className="px-4 py-2">{a.name}</td>
                  <td className="px-4 py-2 text-muted-foreground">{a.actorType}</td>
                  <td className="px-4 py-2 text-right tabular-nums">{a.signalCount}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
