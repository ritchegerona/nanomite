# COPR spec: builds nanomite from the git tag.
# COPR: ritchegerona/nanomite
Name:           nanomite
Version:        0.5.0
Release:        1%{?dist}
Summary:        A microscopic terminal coding agent
License:        MIT
URL:            https://github.com/ritchegerona/nanomite
Source0:        %{url}/archive/v%{version}/nanomite-%{version}.tar.gz
BuildRequires:  cargo
BuildRequires:  git-core
# bundled SQLite via rusqlite "bundled" feature needs a C toolchain
BuildRequires:  gcc

%description
nanomite is a terminal coding agent: streaming chat with any
OpenAI-compatible model, 33 built-in tools (files, shell, git, tests,
symbol index, task plans), persistent memory, a multi-agent board and a
polished ratatui TUI.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release

%install
%{__install} -Dm 755 target/release/nanomite %{buildroot}%{_bindir}/nanomite

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/nanomite

%changelog
* Mon Oct 27 2026 Ritchie Gerona <ritchegerona@users.noreply.github.com> - 0.4.0-1
- v0.4.0: policy engine, git/test/index/plan tools, themed sudo modal
