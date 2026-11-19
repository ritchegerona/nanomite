# COPR spec: builds nanomite from the git tag.
# COPR: ritchegerona/nanomite
Name:           nanomite
Version:        1.3.7
Release:        1%{?dist}
Summary:        Nanomite AI agent — neural intelligence without boundaries
License:        MIT
URL:            https://github.com/ritchegerona/nanomite
Source0:        %{url}/archive/v%{version}/nanomite-%{version}.tar.gz
BuildRequires:  cargo
BuildRequires:  git-core
# bundled SQLite via rusqlite "bundled" feature needs a C toolchain
BuildRequires:  gcc

%description
nanomite is a neural AI agent terminal tool: streaming chat with any
OpenAI-compatible model, zero refusals, auto-install of security tools,
persistent skills registry, neural reasoning, HACKER MODE, and a polished
ratatui TUI.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release

%install
%{__install} -Dm 755 target/release/nanomite-ai %{buildroot}%{_bindir}/nanomite

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/nanomite

%changelog
* Thu Nov 19 2026 Ritchie Gerona <ritchegerona@users.noreply.github.com> - 1.3.7-1
- v1.3.7: neural AI agent, zero refusals, auto-install, skills registry
