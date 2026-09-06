#!/usr/bin/env bash
# Emit the landing page for https://seraphx2.github.io/dev-prompt/ on stdout.
# Env: KEYID (long GPG key id, for the pacman-key lsign line).
set -euo pipefail
KEYID=${KEYID:-<KEYID>}
BASE="https://seraphx2.github.io/dev-prompt"

cat <<HTML
<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>dev-prompt — Linux package repository</title>
<style>
  body{max-width:46rem;margin:3rem auto;padding:0 1.2rem;
       font:15px/1.6 system-ui,sans-serif;color:#1a1a1a;background:#fafafa}
  h1{font-size:1.4rem} h2{font-size:1.05rem;margin-top:2.2rem}
  code,pre{font-family:ui-monospace,Menlo,Consolas,monospace}
  pre{background:#f0f0f0;padding:.9rem 1rem;border-radius:6px;overflow-x:auto;font-size:13px}
  a{color:#0645ad}
  @media(prefers-color-scheme:dark){
    body{color:#e8e8e8;background:#161616}pre{background:#242424}a{color:#8ab4f8}}
</style>

<h1>dev-prompt package repository</h1>
<p><a href="https://github.com/seraphx2/dev-prompt">github.com/seraphx2/dev-prompt</a>
  — command-palette overlay for launching dev repositories.<br>
  Signing key: <a href="dev-prompt.asc">dev-prompt.asc</a> (<code>$KEYID</code>).</p>
<p style="color:#888">Updates arrive through your normal
<code>apt</code> / <code>dnf</code> / <code>pacman -Syu</code>.</p>

<h2>Debian / Ubuntu / Mint</h2>
<pre>curl -fsSL $BASE/dev-prompt.asc | sudo gpg --dearmor -o /usr/share/keyrings/dev-prompt.gpg
echo "deb [signed-by=/usr/share/keyrings/dev-prompt.gpg] $BASE/deb stable main" \\
  | sudo tee /etc/apt/sources.list.d/dev-prompt.list
sudo apt update &amp;&amp; sudo apt install dev-prompt</pre>

<h2>Fedora / RHEL</h2>
<pre>sudo tee /etc/yum.repos.d/dev-prompt.repo &lt;&lt;'EOF'
[dev-prompt]
name=dev-prompt
baseurl=$BASE/rpm
enabled=1
gpgcheck=1
gpgkey=$BASE/dev-prompt.asc
EOF
sudo dnf install dev-prompt</pre>

<h2>Arch / CachyOS / Manjaro</h2>
<p>Import the key, then add the repo to <code>/etc/pacman.conf</code>:</p>
<pre>curl -fsSL $BASE/dev-prompt.asc | sudo pacman-key --add -
sudo pacman-key --lsign-key $KEYID

sudo tee -a /etc/pacman.conf &lt;&lt;'EOF'

[dev-prompt]
SigLevel = Required
Server = $BASE/arch
EOF
sudo pacman -Sy dev-prompt</pre>
HTML
