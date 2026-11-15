# Morph from the GitHub release tarball (prebuilt, no Rust), patched for Nix.
# `desktop = true` keeps the desktop entry and icons; `false` is the
# standalone form with only bin/morph.
{ lib, stdenv, fetchurl, autoPatchelfHook, wrapGAppsHook3, makeWrapper
, webkitgtk_4_1, gtk3, glib, libsoup_3, libxkbcommon, libxcb, libX11, vulkan-loader
, fontconfig, freetype, openssl, zlib, desktop ? true }:

let
  release = lib.importJSON ./release.json;
  src = release.${stdenv.hostPlatform.system} or (throw "morph: no release for ${stdenv.hostPlatform.system}");
in
stdenv.mkDerivation {
  pname = if desktop then "morph" else "morph-standalone";
  inherit (release) version;

  src = fetchurl { inherit (src) url sha256; };

  nativeBuildInputs = [ autoPatchelfHook wrapGAppsHook3 makeWrapper ];
  buildInputs = [ webkitgtk_4_1 gtk3 glib libsoup_3 libxkbcommon libxcb libX11 fontconfig freetype openssl zlib stdenv.cc.cc.lib ];

  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 bin/morph $out/bin/morph
    install -Dm644 LICENSE $out/share/licenses/morph/LICENSE
  '' + lib.optionalString desktop ''
    cp -a share/applications share/icons $out/share/
  '' + ''
    runHook postInstall
  '';

  # GPUI loads Vulkan and xkbcommon-x11 at run time (dlopen).
  preFixup = ''
    gappsWrapperArgs+=(--prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [ vulkan-loader libxkbcommon libX11 libxcb ]})
  '';

  meta = {
    description = "Distributed web browser with local AI (GPUI desktop shell)";
    homepage = "https://github.com/WeirdBrains/morph";
    license = lib.licenses.bsd3;
    platforms = [ "x86_64-linux" ];
    mainProgram = "morph";
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
  };
}
