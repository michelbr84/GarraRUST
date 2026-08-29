package launch

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	"gopkg.in/yaml.v3"

	"github.com/ollama/ollama/cmd/internal/fileutil"
	"github.com/ollama/ollama/envconfig"
)

const (
	// GarraIA's installer accepts flags through the pipe via `sh -s --`.
	// `--skip-setup` suppresses both the interactive wizard and the
	// gateway auto-start, leaving launch to own the configuration.
	garraiaInstallScript = "curl -fsSL https://garraia.org/install.sh | sh -s -- --skip-setup"
	garraiaProviderName  = "Ollama"
	// Launcher-owned key in GarraIA's `llm:` map. Kept distinct from a
	// hand-written `ollama` entry so the two never fight over one config.
	garraiaProviderKey = "ollama-launch"
	// GarraIA reaches Ollama through the OpenAI-compatible surface, so its
	// provider type is `openai` with a `/v1` base URL. The endpoint ignores
	// the key, but the OpenAI client requires a non-empty one.
	garraiaProviderType   = "openai"
	garraiaPlaceholderKey = "ollama"
)

var (
	garraiaGOOS      = runtime.GOOS
	garraiaLookPath  = exec.LookPath
	garraiaCommand   = exec.Command
	garraiaUserHome  = os.UserHomeDir
	garraiaOllamaURL = envconfig.ConnectableHost
)

// Garraia is a managed single-model integration: launch picks one primary
// model and persists the minimum config GarraIA needs to reach Ollama,
// while GarraIA keeps its own `/model` and `/models` UX after startup.
type Garraia struct{}

func (g *Garraia) String() string { return "GarraIA" }

// Supported reports why the integration cannot run on this platform.
//
// GarraIA publishes a POSIX `install.sh` for Linux and macOS only; its
// Windows artifact is a GitHub release binary with no scripted installer,
// so launch has no unattended install path there yet.
func (g *Garraia) Supported() error {
	if garraiaGOOS == "windows" {
		return fmt.Errorf("GarraIA does not publish a Windows install script yet\n\nDownload the binary from https://github.com/michelbr84/GarraRUST/releases and run:\n  garraia config set-model --model <model>\n  garraia chat")
	}
	return nil
}

func (g *Garraia) Run(_ string, _ []LaunchModel, args []string) error {
	// GarraIA reads its primary model from config.yml, which Configure has
	// already written, so the invocation stays plain.
	bin, err := g.binary()
	if err != nil {
		return err
	}
	if len(args) == 0 {
		args = []string{"chat"}
	}
	return garraiaAttachedCommand(bin, args...).Run()
}

// ---------- ManagedSingleModel ------------------------------------

func (g *Garraia) Paths() []string {
	configPath, err := garraiaConfigPath()
	if err != nil {
		return nil
	}
	return []string{configPath}
}

// Configure writes the launcher-owned provider entry and points GarraIA's
// `agent.default_provider` at it.
//
// Deliberately narrow: every other key in config.yml — the operator's
// other providers, channels, voice, system prompt — is round-tripped
// untouched, because this runs unattended and clobbering a config the
// user already filled in would be silent data loss. A previous
// default_provider that still resolves is demoted to the front of
// fallback_providers rather than dropped, so switching to a local model
// does not disable a configured cloud provider outright.
func (g *Garraia) Configure(model string) error {
	configPath, err := garraiaConfigPath()
	if err != nil {
		return err
	}

	cfg := map[string]any{}
	if data, err := os.ReadFile(configPath); err == nil {
		if err := yaml.Unmarshal(data, &cfg); err != nil {
			return fmt.Errorf("parse garraia config: %w", err)
		}
	} else if !os.IsNotExist(err) {
		return err
	}

	llm, _ := cfg["llm"].(map[string]any)
	if llm == nil {
		llm = make(map[string]any)
	}
	entry, _ := llm[garraiaProviderKey].(map[string]any)
	if entry == nil {
		entry = make(map[string]any)
	}
	entry["provider"] = garraiaProviderType
	entry["model"] = model
	entry["api_key"] = garraiaPlaceholderKey
	entry["base_url"] = garraiaBaseURL()
	llm[garraiaProviderKey] = entry
	cfg["llm"] = llm

	agent, _ := cfg["agent"].(map[string]any)
	if agent == nil {
		agent = make(map[string]any)
	}
	previous, _ := agent["default_provider"].(string)
	agent["default_provider"] = garraiaProviderKey
	if fallbacks := garraiaDemotePrevious(agent["fallback_providers"], previous, llm); fallbacks != nil {
		agent["fallback_providers"] = fallbacks
	}
	cfg["agent"] = agent

	data, err := yaml.Marshal(cfg)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(configPath), 0o755); err != nil {
		return err
	}
	if err := fileutil.WriteWithBackup(configPath, data, "garraia"); err != nil {
		return err
	}
	// config.yml can carry `llm.*.api_key`, so GarraIA keeps it at 0600.
	// Match that here rather than leaving it at the process umask.
	return os.Chmod(configPath, 0o600)
}

// CurrentModel reports the model only when GarraIA is actually pointed at
// this launcher's entry and at this Ollama host — otherwise the user has
// since switched providers by hand and launch must not claim ownership.
func (g *Garraia) CurrentModel() string {
	configPath, err := garraiaConfigPath()
	if err != nil {
		return ""
	}
	data, err := os.ReadFile(configPath)
	if err != nil {
		return ""
	}

	cfg := map[string]any{}
	if yaml.Unmarshal(data, &cfg) != nil {
		return ""
	}
	return garraiaManagedCurrentModel(cfg, garraiaBaseURL())
}

func (g *Garraia) Onboard() error { return nil }

func (g *Garraia) RequiresInteractiveOnboarding() bool { return false }

func (g *Garraia) ConfigurationSuccessMessage() string {
	return "GarraIA will use this model. Run `garraia start` for the gateway, or `garraia --model <tag>` for a one-off chat."
}

// ---------- install ------------------------------------------------

func (g *Garraia) installed() bool {
	_, err := g.binary()
	return err == nil
}

func (g *Garraia) ensureInstalled() error {
	if g.installed() {
		return nil
	}
	if err := g.Supported(); err != nil {
		return err
	}

	var missing []string
	for _, dep := range []string{"sh", "curl"} {
		if _, err := garraiaLookPath(dep); err != nil {
			missing = append(missing, dep)
		}
	}
	if len(missing) > 0 {
		return fmt.Errorf("GarraIA is not installed and required dependencies are missing\n\nInstall the following first:\n  %s\n\nThen re-run:\n  ollama launch garraia", strings.Join(missing, "\n  "))
	}

	ok, err := ConfirmPrompt("GarraIA is not installed. Install now?")
	if err != nil {
		return err
	}
	if !ok {
		return fmt.Errorf("garraia installation cancelled")
	}

	fmt.Fprintf(os.Stderr, "\nInstalling GarraIA...\n")
	if err := g.runInstallScript(); err != nil {
		return fmt.Errorf("failed to install garraia: %w", err)
	}
	if !g.installed() {
		return fmt.Errorf("garraia was installed but the binary was not found on PATH\n\nYou may need to restart your shell")
	}

	fmt.Fprintf(os.Stderr, "%sGarraIA installed successfully%s\n\n", ansiGreen, ansiReset)
	return nil
}

func (g *Garraia) runInstallScript() error {
	return garraiaAttachedCommand("sh", "-c", garraiaInstallScript).Run()
}

// binary resolves the CLI. The installer prefers ~/.local/bin when it is on
// PATH and falls back to /usr/local/bin, so both are probed. A `cargo
// install` build produces `garra` instead of `garraia` — same binary, and
// people who built from source have only that name.
func (g *Garraia) binary() (string, error) {
	for _, name := range []string{"garraia", "garra"} {
		if path, err := garraiaLookPath(name); err == nil {
			return path, nil
		}
	}

	home, err := garraiaUserHome()
	if err != nil {
		return "", err
	}
	for _, fallback := range []string{
		filepath.Join(home, ".local", "bin", "garraia"),
		"/usr/local/bin/garraia",
	} {
		if _, err := os.Stat(fallback); err == nil {
			return fallback, nil
		}
	}

	return "", fmt.Errorf("garraia is not installed")
}

// ---------- helpers -------------------------------------------------

func garraiaAttachedCommand(name string, args ...string) *exec.Cmd {
	cmd := garraiaCommand(name, args...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd
}

// garraiaConfigDir mirrors garraia_config::loader::default_config_dir:
// an explicit override wins; otherwise the XDG path is canonical and the
// legacy ~~.garraia is used only when it already exists.
func garraiaConfigDir() (string, error) {
	if dir := strings.TrimSpace(os.Getenv("GARRAIA_CONFIG_DIR")); dir != "" {
		return filepath.Clean(dir), nil
	}
	home, err := garraiaUserHome()
	if err != nil {
		return "", err
	}
	xdg := filepath.Join(home, ".config", "garraia")
	if _, err := os.Stat(xdg); err == nil {
		return xdg, nil
	}
	legacy := filepath.Join(home, ".garraia")
	if _, err := os.Stat(legacy); err == nil {
		return legacy, nil
	}
	return xdg, nil
}

func garraiaConfigPath() (string, error) {
	dir, err := garraiaConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "config.yml"), nil
}

func garraiaBaseURL() string {
	return strings.TrimRight(garraiaOllamaURL().String(), "/") + "/v1"
}

func garraiaNormalizeURL(raw string) string {
	return strings.TrimRight(strings.TrimSpace(strings.ToLower(raw)), "/")
}

// garraiaDemotePrevious puts a displaced default_provider at the front of
// fallback_providers, when it names an entry that still exists and is not
// already listed. Returns nil when nothing should change.
func garraiaDemotePrevious(existing any, previous string, llm map[string]any) []any {
	previous = strings.TrimSpace(previous)
	if previous == "" || previous == garraiaProviderKey {
		return nil
	}
	if _, ok := llm[previous]; !ok {
		return nil
	}

	current, _ := existing.([]any)
	for _, item := range current {
		if name, ok := item.(string); ok && strings.TrimSpace(name) == previous {
			return nil
		}
	}
	return append([]any{previous}, current...)
}

func garraiaManagedCurrentModel(cfg map[string]any, baseURL string) string {
	agent, _ := cfg["agent"].(map[string]any)
	if agent == nil {
		return ""
	}
	def, _ := agent["default_provider"].(string)
	if strings.TrimSpace(def) != garraiaProviderKey {
		return ""
	}

	llm, _ := cfg["llm"].(map[string]any)
	if llm == nil {
		return ""
	}
	entry, _ := llm[garraiaProviderKey].(map[string]any)
	if entry == nil {
		return ""
	}

	configBaseURL, _ := entry["base_url"].(string)
	if garraiaNormalizeURL(configBaseURL) != garraiaNormalizeURL(baseURL) {
		return ""
	}

	model, _ := entry["model"].(string)
	return strings.TrimSpace(model)
}
