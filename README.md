# JBL Headphones

App de bandeja para fones JBL no Windows, Linux e macOS: bateria, cancelamento de ruído,
equalizador com os presets da JBL, gestos, localizar o fone e os ajustes do próprio fone,
sem precisar do app do celular.

## Baixar

[Última versão](https://github.com/edsuuu/jbl-headphones/releases/latest):

- **Windows:** `jbl-headphones-windows-x64.zip`. Extraia e abra `jbl-headphones.exe`.
- **Linux:** `jbl-headphones-linux-x64.tar.gz`. Extraia e rode `./jbl-headphones` (precisa do BlueZ).
- **macOS:** `jbl-headphones-mac.zip`. Arraste `JBL Headphones.app` para Aplicativos. O app não é
  assinado pela Apple: na primeira vez, clique nele com o botão direito e escolha **Abrir**.

O app se atualiza sozinho ao abrir; dá para desligar no botão de ajustes, no topo da lista de fones.

## Fones

Ele mostra só os fones JBL conectados a este computador. Os 44 modelos em `devices/` foram gerados
do app oficial da JBL por `tools/import_jbl_apk.py`; o testado de ponta a ponta é o Tune Flex 2.

## Desenvolver

- Windows e Linux (Rust): `cargo run` (`cargo run -- --debug` mostra cada pacote trocado com o fone).
- macOS (Swift, em `mac/`): `mac/run.sh`.
- Perfis e fotos: `python3 tools/import_jbl_apk.py <JBL Headphones .apkm>`.

Uma tag `v<versão do Cargo.toml>` publica a versão: o CI compila os três sistemas e cria a release.

---

Projeto independente, sem relação com a JBL ou a Harman. JBL é marca da Harman International.
