# Premiers pas {#erste-schritte}

Manta est un gestionnaire de fichiers pour Mac qui affiche plusieurs sous-fenêtres côte à côte. Ce chapitre explique comment installer Manta, ce qui se passe au premier lancement et pourquoi macOS vous demande des autorisations.

![Manta avec deux sous-fenêtres et la barre latérale](images/overview-two-panes.png)

## Configuration requise {#voraussetzungen}

- macOS 11 (Big Sur) ou ultérieur.
- Un Mac avec puce Apple (M1 ou ultérieure).

## Installer Manta {#installieren}

1. Double-cliquez sur `Manta-<version>.dmg`.
2. Dans la fenêtre qui s’ouvre, faites glisser **Manta** sur le dossier **Applications**.
3. Éjectez l’image disque (cliquez sur l’icône d’éjection dans le Finder).
4. Lancez Manta depuis le dossier **Applications**, par exemple avec Launchpad ou Spotlight.

> [!IMPORTANT]
> Lancez toujours Manta depuis le dossier **Applications**, et non directement depuis l’image disque. C’est seulement ainsi que Manta peut se mettre à jour ([Mises à jour](17-updates.md)) et devenir votre gestionnaire de fichiers par défaut.

## Le premier lancement : Gatekeeper {#gatekeeper}

Si votre copie de Manta n’est pas notariée par Apple, macOS bloque le premier lancement avec un message tel que « “Manta” n’a pas été ouvert ». Il s’agit d’une fonction de sécurité de macOS (Gatekeeper), pas d’un bug de Manta.

Pour ouvrir Manta malgré tout :

1. Cliquez sur **Terminé** dans le message (et non sur « Placer dans la corbeille »).
2. Ouvrez **Réglages Système** → **Confidentialité et sécurité**.
3. Faites défiler jusqu’à la section **Sécurité**. Une remarque y concerne Manta.
4. Cliquez sur **Ouvrir quand même** et confirmez avec votre mot de passe ou Touch ID.

Sur les versions antérieures à macOS 15, il suffit de cliquer sur Manta dans le Finder en maintenant la touche Contrôle enfoncée, puis de choisir **Ouvrir**.

Vous n’avez à le faire qu’une seule fois. Voir aussi [Dépannage](19-fehlerbehebung.md#gatekeeper).

## Pourquoi macOS demande des autorisations {#berechtigungen}

macOS protège certains dossiers et certaines fonctions. Chaque app doit demander une fois l’autorisation avant de les utiliser – un gestionnaire de fichiers aussi. Les demandes n’apparaissent pas toutes au lancement : chacune s’affiche la première fois que vous ouvrez le dossier ou utilisez la fonction concernés.

| macOS demande l’accès à… | Quand | Pourquoi Manta en a besoin |
|---|---|---|
| Les dossiers Bureau, Documents et Téléchargements | À la première ouverture du dossier | Pour afficher et gérer les fichiers qu’il contient. |
| Volumes amovibles, volumes réseau | À la première ouverture d’une clé USB ou d’un partage réseau | Même chose pour les disques externes et les partages de serveur. |
| Réseau local | À la première connexion à un serveur de votre réseau | Pour les serveurs SFTP, FTP et S3 de votre réseau local (un NAS, par exemple). |
| Automatisation (Finder, Terminal) | Lorsqu’une fonction pilote le Finder ou Terminal | Par exemple pour placer des éléments dans la corbeille (Doublons), afficher des fichiers dans le Finder ou exécuter des actions rapides comme « Ouvrir dans Terminal.app ». |

Si vous cliquez sur « Refuser », Manta affiche une remarque lorsque vous ouvrez le dossier. Vous pouvez accorder l’accès plus tard : **Réglages Système** → **Confidentialité et sécurité** → **Fichiers et dossiers** (ou **Automatisation**, **Réseau local**).

> [!TIP]
> Certains dossiers (comme `~/Library/Mail` ou les données Time Machine) ne s’ouvrent qu’avec l’**Accès complet au disque**. Accordez-le dans **Réglages Système** → **Confidentialité et sécurité** → **Accès complet au disque**. C’est facultatif ; la plupart des utilisateurs n’en ont pas besoin.

> [!NOTE]
> Manta ne collecte aucune donnée d’utilisation. Il ne se connecte à Internet que pour les connexions que vous ouvrez vous-même et pour rechercher les mises à jour (voir [Mises à jour et confidentialité](17-updates.md#datenschutz)).

## Gestionnaire de fichiers par défaut {#standard-dateimanager}

Lors des premiers lancements, Manta demande : **Utiliser Manta comme gestionnaire de fichiers par défaut ?**

- **Définir par défaut** : « Afficher dans le Finder » dans les autres apps ouvre alors Manta, tout comme les dossiers ouverts par d’autres apps ou par Terminal (par exemple `open ~/Downloads`).
- **Plus tard** : Manta reposera la question lors d’un prochain lancement (trois fois au maximum).
- **Ne plus demander** : Manta ne pose plus jamais la question.

Vous pouvez modifier ce choix à tout moment dans **Réglages** → **Général** → **Gestionnaire de fichiers par défaut**, ou avec **Gestionnaire de fichiers par défaut…** dans la palette de commandes.

![La question sur le gestionnaire de fichiers par défaut](images/default-file-manager-prompt.png)

Ce que macOS permet – et ce qu’il ne permet pas :

- Le Bureau, l’icône du Finder dans le Dock et les doubles-clics dans les fenêtres du Finder restent **toujours** dans le Finder. macOS ne permet à aucune app de remplacer complètement le Finder.
- À partir de macOS 26, macOS ne confie les dossiers ouverts par d’autres apps qu’au Finder. Manta ne prend alors en charge que « Afficher dans le Finder » ; les Réglages vous l’indiquent.
- Si vous désactivez l’option, le rôle revient à l’app précédente (un autre gestionnaire de fichiers, par exemple) ou au Finder.
- L’option n’est disponible que si Manta est exécuté depuis **Applications** (ou `~/Applications`).

## La fenêtre en un coup d’œil {#ueberblick}

- **Barre d’outils** en haut : Nouveau dossier, Renommer, Supprimer, Copier/Déplacer dans l’autre sous-fenêtre, fichiers masqués, Comparer et synchroniser, Transferts, Se connecter, Commandes et Réglages.
- **Barre latérale** à gauche : Favoris, Emplacements, Appareils, Serveurs et Conteneurs (voir [Favoris et barre latérale](07-favoriten-seitenleiste.md)).
- **Sous-fenêtres** : une ou plusieurs listes de fichiers côte à côte, chacune avec ses onglets et sa barre du chemin d’accès (voir [Sous-fenêtres et onglets](02-bereiche-tabs.md)).
- **Barre d’état** en bas : le nombre d’éléments, la sélection, les transferts en cours, la charge du système (CPU, GPU, RAM ; voir [Charge du système](14-plugins.md#systemauslastung)) et un rappel de la palette de commandes.

## La palette de commandes {#befehlspalette}

Toutes les commandes de Manta se trouvent dans la **palette de commandes** – celles des plug-ins aussi.

1. Appuyez sur **⇧⌘P** (ou **⌘P**).
2. Tapez quelques lettres, par exemple `nouv` ou `sftp`.
3. Choisissez avec ↑ et ↓, puis appuyez sur ↩.

La palette affiche le raccourci de chaque commande à côté de celle-ci : vous apprenez ainsi les raccourcis au fil de l’utilisation.

![Palette de commandes avec une recherche](images/command-palette.png)

> [!NOTE]
> Manta n’a que le menu **Manta** dans la barre des menus (À propos de Manta, Rechercher les mises à jour…, Réglages…, Quitter). Toutes les autres commandes se trouvent dans la barre d’outils, le menu contextuel (clic droit) et la palette de commandes.

## Étapes suivantes

- [Sous-fenêtres et onglets](02-bereiche-tabs.md) : travailler avec plusieurs sous-fenêtres.
- [Gérer les fichiers](05-dateien.md) : copier, déplacer, renommer, supprimer.
- [Raccourcis clavier](18-tastenkuerzel.md) : tous les raccourcis en un coup d’œil.
