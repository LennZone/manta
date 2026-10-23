# Vos propres plug-ins {#eigene-plugins}

Vous pouvez enrichir Manta avec vos propres plug-ins – dans n’importe quel langage de programmation. Ce chapitre explique ce qu’un plug-in peut faire, comment l’installer, le mettre à jour et en trouver, et à quoi faire attention. Le guide technique se trouve dans la documentation pour les développeurs.

## Ce que peuvent faire les plug-ins {#moeglichkeiten}

- Afficher des **pastilles** à côté des noms de fichier (comme les pastilles Git).
- Afficher des **pastilles** dans la barre du chemin d’accès (comme la branche ou le projet).
- Proposer des **commandes** pour la palette de commandes et le menu contextuel, avec un raccourci si vous le souhaitez.
- Afficher des **panneaux** sous une sous-fenêtre, dans le panneau latéral droit ou sous forme de zone de dialogue.
- Afficher des **éléments dans la barre d’état**, avec une icône, une petite jauge et une couleur d’alerte (comme [Charge du système](14-plugins.md#systemauslastung)).
- Afficher des **messages**.

Un plug-in est un dossier contenant un fichier `plugin.toml` et un programme (un script Python, par exemple). Manta lance le programme et échange des messages avec lui (lignes JSON via stdin/stdout). Un fichier `.mantaplugin` n’est rien d’autre que ce dossier compressé au format ZIP.

## Installer un plug-in {#installieren}

1. Ouvrez **Réglages** (**⌘,**) → **Plug-ins**.
2. Cliquez sur **Installer un plug-in…** et choisissez **À partir d’un fichier (.mantaplugin, .zip)…** ou **À partir d’un dossier…** (celui qui contient `plugin.toml`).
3. Avant toute installation, Manta affiche un récapitulatif : nom, version, auteur, description, **Ce qu’il déclare faire** (par exemple « Lit vos fichiers », « Modifie ou supprime des fichiers », « Utilise le réseau », « Lance d’autres programmes », « Exécute des commandes dans le Terminal »), la **Commande** exécutée, la **Source**, un lien vers le **Code source**, la **Licence** et la taille. En dessous, un avertissement rappelle que Manta ne vérifie pas les plug-ins.
4. Cliquez sur **Installer**. Le plug-in apparaît alors sous **Installés** – encore désactivé.

![Récapitulatif avant l’installation d’un plug-in](images/plugin-install.png)

> [!NOTE]
> Ce qu’un plug-in déclare faire est indiqué par son auteur. Manta ne peut ni le vérifier ni l’imposer.

### L’activer {#einschalten}

Cliquez sur **Activer** sur la carte du plug-in. La première fois, Manta vous demande de confirmer et affiche à nouveau l’auteur, la version, les autorisations, la commande et le dossier dans lequel elle s’exécute.

![Confirmation lors de l’activation d’un plug-in externe](images/plugin-confirm.png)

Si vous avez téléchargé le plug-in sur Internet, macOS place ses fichiers en quarantaine et Gatekeeper bloquerait les programmes qu’il contient. La confirmation le signale : c’est seulement lorsque vous activez le plug-in que Manta retire l’attribut de quarantaine de ses fichiers pour que ses programmes puissent s’exécuter – jamais à votre insu.

Autres boutons sur la carte d’un plug-in : **Recharger**, **Afficher le journal**, **Afficher le dossier**, **Désinstaller**. En haut : **Ouvrir le dossier des plug-ins** et **Recharger** (recherche les plug-ins ajoutés, modifiés ou supprimés).

## Mettre à jour et désinstaller {#aktualisieren}

- **Mettre à jour** : installez la nouvelle version de la même manière, à partir d’un fichier ou d’un dossier. Si elle a le même identifiant qu’un plug-in installé, le récapitulatif indique une mise à jour et affiche sous **Installé** la version qui sera remplacée. Après confirmation, Manta remplace le plug-in ; son état activé ou désactivé est conservé.
- **Désinstaller** : cliquez sur **Désinstaller** sur la carte du plug-in. Manta supprime le dossier du plug-in ; son dossier de données est conservé.

## Trouver des plug-ins {#entdecken}

Dans **Réglages** → **Plug-ins**, cliquez sur **Parcourir les plug-ins** en haut à droite (ou choisissez **Parcourir les plug-ins…** dans la palette de commandes). Le répertoire des plug-ins s’ouvre sur [manta-filemanager.com/plugins](https://manta-filemanager.com/plugins/). Il liste les plug-ins intégrés et ceux de la communauté. Chaque entrée comporte un lien vers son code source et une somme de contrôle SHA-256 du téléchargement. Téléchargez le fichier `.mantaplugin` et installez-le comme décrit [plus haut](#installieren).

## Sécurité {#sicherheit}

> [!WARNING]
> Les plug-ins externes s’exécutent comme des programmes avec **les autorisations de votre utilisateur** : ils peuvent lire, modifier et supprimer vos fichiers, lancer d’autres programmes et envoyer des données sur le réseau. Manta ne vérifie pas les plug-ins et ne les isole pas dans un bac à sable – pas même ceux du répertoire des plug-ins. N’installez que des plug-ins de confiance, et lisez le code des plug-ins provenant d’Internet avant de les activer.

C’est pourquoi Manta affiche un récapitulatif avant l’installation, les nouveaux plug-ins démarrent désactivés et Manta vous demande confirmation la première fois que vous en activez un.

## Quand un plug-in ne fonctionne pas {#probleme}

- La carte affiche **Erreur** et la raison, par exemple lorsque `python3` est absent.
- **Afficher le journal** affiche les derniers messages du plug-in.
- Si un plug-in plante, Manta le redémarre – trois fois par minute au maximum. Ensuite, il reste en état d’erreur jusqu’à ce que vous cliquiez sur **Recharger**.
- Si le dossier des plug-ins contient deux dossiers avec le même identifiant de plug-in, le second affiche **Chargement impossible** et un message. Supprimez l’un des deux.

## Emplacement des plug-ins {#speicherorte}

| Quoi | Où |
|---|---|
| Plug-ins installés | `~/Library/Application Support/Manta/plugins/<id>/` |
| Données et journal d’un plug-in | `~/Library/Application Support/Manta/plugin-data/<id>/` (journal : `plugin.log`) |
| État activé/désactivé | `~/Library/Application Support/Manta/plugins.json` |

La désinstallation supprime le dossier du plug-in ; son dossier de données est conservé.

## Écrire vous-même un plug-in {#schreiben}

Le moyen le plus rapide de commencer est l’exemple **Hello Python** (`examples/plugins/hello-python` dans le code source, commenté ligne par ligne, uniquement avec la bibliothèque standard de Python). Vous pouvez aussi l’installer depuis **Réglages** → **Plug-ins** → **Disponibles**. Il existe aussi des modèles pour Node.js, le shell (bash + jq) et Rust (`examples/plugins/`) ; pour Rust, il y a en plus le crate `manta-plugin-protocol`.

Guide, référence du protocole et exemples : [Écrire des plug-ins pour Manta (docs/plugins.md)](../../plugins.md).

Les plug-ins peuvent parler toutes les langues de Manta : Manta leur indique la langue actuelle, et `plugin.toml` peut contenir un nom et une description traduits (`[localized.fr]`, `[localized.de]`, etc.).

## Développer et tester {#entwickeln}

- **Recharger les plug-ins** (palette de commandes) redémarre tous les plug-ins externes en cours d’exécution et analyse à nouveau le dossier des plug-ins.
- **Mode développeur** : lancez Manta depuis le Terminal avec
  `/Applications/Manta.app/Contents/MacOS/manta --plugin-dev ~/code/my-plugin`
  (répétable, ou via la variable d’environnement `MANTA_PLUGIN_DEV=dir1:dir2`). Pour cette session, le plug-in s’exécute directement depuis ce dossier – approuvé, activé et marqué **Développement**. Une copie installée avec le même identifiant s’efface pendant ce temps ; rien n’est enregistré.
- **Tester sans Manta** : `python3 scripts/plugin-test.py chemin/vers/plugin`.
- **Journal** : **Afficher le journal** sur la carte, ou le fichier `~/Library/Application Support/Manta/plugin-data/<id>/plugin.log`.

Pour en savoir plus : [Develop and debug (docs/plugins.md, en anglais)](../../plugins.md#develop-and-debug).

## Publier un plug-in {#veroeffentlichen}

Placez le plug-in dans un dépôt public sous une licence approuvée par l’OSI, joignez un fichier `.mantaplugin` à une release et ouvrez une pull request qui l’ajoute à `plugins/registry.json`. Il apparaît ensuite dans **Parcourir les plug-ins**. Détails : [Publishing a plugin (docs/plugins.md, en anglais)](../../plugins.md#publishing-a-plugin).
