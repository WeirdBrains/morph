# Built by scripts/linux-packages.sh from the release tarball (prebuilt; no
# Rust needed). It defines pkg (morph or morph-standalone), pkgver, form
# (desktop or standalone), payload (the file tree) and filelist.
Name:           %{pkg}
Version:        %{pkgver}
Release:        1
Summary:        Distributed web browser with local AI (GPUI desktop shell)
License:        BSD-3-Clause
URL:            https://github.com/WeirdBrains/morph
Vendor:         Weird Brains LLC
Requires:       webkit2gtk4.1
Requires:       gtk3
Requires:       libxkbcommon-x11
Requires:       vulkan-loader
Requires:       fontconfig
Requires:       freetype
Requires:       libstdc++
%if "%{form}" == "standalone"
Conflicts:      morph
Provides:       morph = %{version}-%{release}
%else
Conflicts:      morph-standalone
%endif
AutoReqProv:    no

%global debug_package %{nil}
%global source_date_epoch_from_changelog 0
%global __strip /bin/true

%description
Morph is a distributed web browser with local AI. %{?form:This is the %{form} form.}

%install
cp -a %{payload}/. %{buildroot}/

%files -f %{filelist}
