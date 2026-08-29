package launch

import (
	"net/url"
	"os"
	"path/filepath"
	"testing"

	"gopkg.in/yaml.v3"
)

func withGarraiaPlatform(t *testing.T, goos string) {
	t.Helper()
	old := garraiaGOOS
	garraiaGOOS = goos
	t.Cleanup(func() { garraiaGOOS = old })
}

func withGarraiaOllamaURL(t *testing.T, rawURL string) {
	t.Helper()
	old := garraiaOllamaURL
	garraiaOllamaURL = func() *url.URL {
		u, err := url.Parse(rawURL)
		if err != nil {
			t.Fatalf("parse test Ollama URL: %v", err)
		}
		return u
	}
	t.Cleanup(func() { garraiaOllamaURL = old })
}

// withGarraiaConfigDir points the integration at a temp dir through the
// same env var GarraIA itself honors, so the tests exercise the real
// lookup rather than a shim.
func withGarraiaConfigDir(t *testing.T, dir string) {
	t.Helper()
	t.Setenv("GARRAIA_CONFIG_DIR", dir)
}

func readGarraiaConfig(t *testing.T, path string) map[string]any {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read config: %v", err)
	}
	cfg := map[string]any{}
	if err := yaml.Unmarshal(data, &cfg); err != nil {
		t.Fatalf("parse config: %v", err)
	}
	return cfg
}

func garraiaEntry(t *testing.T, cfg map[string]any) map[string]any {
	t.Helper()
	llm, ok := cfg["llm"].(map[string]any)
	if !ok {
		t.Fatalf("config has no llm map: %#v", cfg)
	}
	entry, ok := llm[garraiaProviderKey].(map[string]any)
	if !ok {
		t.Fatalf("config has no %q entry: %#v", garraiaProviderKey, llm)
	}
	return entry
}

func TestGarraiaConfigureWritesManagedEntry(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	g := &Garraia{}
	if err := g.Configure("qwen3.8:latest"); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	cfg := readGarraiaConfig(t, filepath.Join(dir, "config.yml"))
	entry := garraiaEntry(t, cfg)

	// GarraIA reaches Ollama over the OpenAI-compatible surface.
	if got := entry["provider"]; got != garraiaProviderType {
		t.Errorf("provider = %v, want %v", got, garraiaProviderType)
	}
	if got := entry["model"]; got != "qwen3.8:latest" {
		t.Errorf("model = %v, want qwen3.8:latest", got)
	}
	if got := entry["base_url"]; got != "http://127.0.0.1:11434/v1" {
		t.Errorf("base_url = %v, want .../v1", got)
	}
	// The endpoint ignores the key, but the OpenAI client demands one.
	if got := entry["api_key"]; got != garraiaPlaceholderKey {
		t.Errorf("api_key = %v, want %v", got, garraiaPlaceholderKey)
	}

	agent, _ := cfg["agent"].(map[string]any)
	if agent == nil || agent["default_provider"] != garraiaProviderKey {
		t.Errorf("default_provider = %#v, want %v", agent, garraiaProviderKey)
	}
}

func TestGarraiaCurrentModelRoundTrips(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	g := &Garraia{}
	if got := g.CurrentModel(); got != "" {
		t.Errorf("CurrentModel with no config = %q, want empty", got)
	}
	if err := g.Configure("qwen3:8b"); err != nil {
		t.Fatalf("Configure: %v", err)
	}
	if got := g.CurrentModel(); got != "qwen3:8b" {
		t.Errorf("CurrentModel = %q, want qwen3:8b", got)
	}
}

// A config pointed at a different Ollama host is not this launcher's to
// claim — reporting a model there would make `--restore` and the model
// picker act on state launch does not own.
func TestGarraiaCurrentModelIgnoresForeignHost(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	g := &Garraia{}
	if err := g.Configure("qwen3:8b"); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	withGarraiaOllamaURL(t, "http://other-host:11434")
	if got := g.CurrentModel(); got != "" {
		t.Errorf("CurrentModel across hosts = %q, want empty", got)
	}
}

// Likewise when the user has since pointed GarraIA at another provider
// by hand: the managed entry may still exist, but it is not the default.
func TestGarraiaCurrentModelIgnoresForeignDefaultProvider(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	g := &Garraia{}
	if err := g.Configure("qwen3:8b"); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	path := filepath.Join(dir, "config.yml")
	cfg := readGarraiaConfig(t, path)
	cfg["agent"].(map[string]any)["default_provider"] = "openrouter"
	data, err := yaml.Marshal(cfg)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("write: %v", err)
	}

	if got := g.CurrentModel(); got != "" {
		t.Errorf("CurrentModel = %q, want empty when another provider is default", got)
	}
}

// The unattended-install case: an operator already configured a cloud
// provider, then a launcher points GarraIA at a local model. Their
// config must survive intact and the cloud provider must live on as a
// fallback rather than being dropped.
func TestGarraiaConfigurePreservesExistingConfig(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	existing := map[string]any{
		"llm": map[string]any{
			"openrouter": map[string]any{
				"provider": "openrouter",
				"model":    "openrouter/auto",
				"api_key":  "chave-do-operador",
			},
		},
		"agent": map[string]any{
			"default_provider": "openrouter",
			"system_prompt":    "persona do operador",
		},
		"gateway": map[string]any{"port": 4242},
	}
	data, err := yaml.Marshal(existing)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	path := filepath.Join(dir, "config.yml")
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("write: %v", err)
	}

	if err := (&Garraia{}).Configure("qwen3.8:latest"); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	cfg := readGarraiaConfig(t, path)
	llm := cfg["llm"].(map[string]any)
	other := llm["openrouter"].(map[string]any)
	if other["api_key"] != "chave-do-operador" || other["model"] != "openrouter/auto" {
		t.Errorf("existing provider was modified: %#v", other)
	}

	agent := cfg["agent"].(map[string]any)
	if agent["system_prompt"] != "persona do operador" {
		t.Errorf("system_prompt was lost: %#v", agent)
	}
	if agent["default_provider"] != garraiaProviderKey {
		t.Errorf("default_provider = %v, want %v", agent["default_provider"], garraiaProviderKey)
	}
	fallbacks, _ := agent["fallback_providers"].([]any)
	if len(fallbacks) != 1 || fallbacks[0] != "openrouter" {
		t.Errorf("fallback_providers = %#v, want [openrouter]", fallbacks)
	}
	if gw, _ := cfg["gateway"].(map[string]any); gw == nil || gw["port"] != 4242 {
		t.Errorf("gateway was lost: %#v", cfg["gateway"])
	}
}

// Re-running must be idempotent: no duplicate entries, and the key must
// never end up as its own fallback.
func TestGarraiaConfigureIsIdempotent(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	g := &Garraia{}
	for _, model := range []string{"qwen3.8:latest", "qwen3:8b", "qwen3:8b"} {
		if err := g.Configure(model); err != nil {
			t.Fatalf("Configure(%s): %v", model, err)
		}
	}

	cfg := readGarraiaConfig(t, filepath.Join(dir, "config.yml"))
	if got := len(cfg["llm"].(map[string]any)); got != 1 {
		t.Errorf("llm entries = %d, want 1", got)
	}
	if got := garraiaEntry(t, cfg)["model"]; got != "qwen3:8b" {
		t.Errorf("model = %v, want qwen3:8b", got)
	}
	agent := cfg["agent"].(map[string]any)
	if fallbacks, _ := agent["fallback_providers"].([]any); len(fallbacks) != 0 {
		t.Errorf("fallback_providers = %#v, want empty", fallbacks)
	}
}

// config.yml can carry `llm.*.api_key`, so it must not be left at the
// process umask.
func TestGarraiaConfigureHardensPermissions(t *testing.T) {
	if garraiaGOOS == "windows" {
		t.Skip("POSIX permissions only")
	}
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)
	withGarraiaOllamaURL(t, "http://127.0.0.1:11434")

	if err := (&Garraia{}).Configure("qwen3.8:latest"); err != nil {
		t.Fatalf("Configure: %v", err)
	}
	info, err := os.Stat(filepath.Join(dir, "config.yml"))
	if err != nil {
		t.Fatalf("stat: %v", err)
	}
	if perm := info.Mode().Perm(); perm != 0o600 {
		t.Errorf("config mode = %o, want 600", perm)
	}
}

func TestGarraiaPathsNamesTheConfig(t *testing.T) {
	dir := t.TempDir()
	withGarraiaConfigDir(t, dir)

	paths := (&Garraia{}).Paths()
	want := filepath.Join(dir, "config.yml")
	if len(paths) != 1 || paths[0] != want {
		t.Errorf("Paths() = %#v, want [%s]", paths, want)
	}
}

func TestGarraiaSupportedRejectsWindows(t *testing.T) {
	withGarraiaPlatform(t, "windows")
	if err := (&Garraia{}).Supported(); err == nil {
		t.Error("Supported() on windows = nil, want an error naming the manual path")
	}

	withGarraiaPlatform(t, "linux")
	if err := (&Garraia{}).Supported(); err != nil {
		t.Errorf("Supported() on linux = %v, want nil", err)
	}
}

func TestGarraiaIsRegistered(t *testing.T) {
	spec, err := LookupIntegrationSpec("garraia")
	if err != nil {
		t.Fatalf("garraia is not in the integration registry: %v", err)
	}
	if _, ok := spec.Runner.(*Garraia); !ok {
		t.Errorf("runner = %T, want *Garraia", spec.Runner)
	}
	if spec.Install.CheckInstalled == nil || spec.Install.EnsureInstalled == nil {
		t.Error("install spec is missing its hooks")
	}
}
