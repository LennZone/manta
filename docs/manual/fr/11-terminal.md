# Terminal {#terminal}

Sous chaque sous-fenêtre, vous pouvez afficher un terminal qui suit le dossier de la sous-fenêtre – et inversement. Il s’exécute là où se trouve la sous-fenêtre : sur votre Mac, sur un serveur SFTP ou dans un conteneur.

![Une sous-fenêtre avec un terminal en dessous](images/terminal.png)

## Afficher le terminal {#einblenden}

- Appuyez sur **⌃`** (**Afficher/masquer le Terminal**).
- Ou choisissez **Afficher/masquer le Terminal** dans la palette de commandes.

Le terminal apparaît sous la sous-fenêtre active. Faites glisser son bord supérieur pour modifier sa hauteur.

Autres commandes dans la palette de commandes :

- **Nouvelle session Terminal** – démarre un nouveau shell.
- **Effacer le Terminal** – efface l’affichage.

Si vous mettez fin au shell (avec `exit`, par exemple), le terminal affiche « [Processus terminé — appuyez sur Retour pour redémarrer] ».

## La sous-fenêtre et le terminal se suivent mutuellement {#folgen}

**De la sous-fenêtre vers le terminal :** lorsque vous changez de dossier dans la sous-fenêtre, le shell exécute `cd` vers ce dossier – dès qu’il est inactif, c’est-à-dire lorsqu’aucune commande n’est en cours. D’ici là, une remarque indique « Ira dans « dossier » dès que le shell sera prêt ». Ce que vous avez commencé à taper est conservé.

**Du terminal vers la sous-fenêtre :** tapez `cd ~/Projects` dans le terminal et la sous-fenêtre au-dessus affiche ce dossier. Désactivez ce comportement avec **La sous-fenêtre suit « cd » du terminal** (voir [Réglages](#einstellungen)).

Votre invite et la configuration de votre shell restent inchangées.

## Terminal dans les conteneurs et sur les serveurs {#container-ssh}

Le terminal s’exécute là où se trouve la sous-fenêtre :

| La sous-fenêtre affiche… | Terminal |
|---|---|
| un dossier local (y compris les dossiers cloud) | votre shell habituel sur le Mac |
| un conteneur Docker | un shell à l’intérieur du conteneur (`docker exec`) |
| un pod Kubernetes | un shell à l’intérieur du pod (`kubectl exec`) |
| un serveur SFTP | `ssh` vers le serveur, avec votre configuration SSH et ssh-agent |
| S3, FTP, une archive | pas de terminal (« Aucun terminal pour cet emplacement ») |

- Manta n’installe rien dans le conteneur ni sur le serveur pour cela.
- Pour les connexions SFTP avec mot de passe, vous saisissez le mot de passe dans le terminal. Un fichier de clé qui ne figure ni dans votre configuration SSH ni dans votre agent est également demandé dans le terminal.
- Chaque sous-fenêtre conserve jusqu’à quatre shells (local, conteneurs, serveurs). Lorsque vous revenez en arrière, le shell précédent est de nouveau là, avec son historique.
- Si un conteneur est arrêté, le terminal indique « Le conteneur « nom » est arrêté ». Démarrez-le et cliquez sur la remarque.

Clic droit → **Ouvrir un shell dans le conteneur** démarre un shell dans le dossier du conteneur que vous consultez.

## Actions sur fichier dans le terminal {#dateiaktionen}

Lorsque vous copiez, déplacez, supprimez ou renommez quelque chose dans Manta, le terminal affiche la commande shell correspondante – comme `cp -R …`, `scp …`, `docker cp …` ou `aws s3 cp …` – suivie du résultat (« terminé », « échec : … »). Le placement dans la corbeille apparaît sous la forme `mv … ~/.Trash/`, une suppression définitive sous la forme `rm -r …`.

> [!NOTE]
> Il s’agit uniquement d’un affichage. Manta effectue le travail lui-même ; rien n’est exécuté dans le shell. Vous pouvez toutefois copier les commandes, par exemple pour vos propres scripts.

Dans les shells distants (conteneurs, SSH), Manta affiche ces commandes sous forme de carte par-dessus le terminal au lieu de les écrire dans le shell.

Pour désactiver cette fonction : **Afficher les actions sur fichier dans le terminal** dans les réglages.

## Réglages {#einstellungen}

Ouvrez **Réglages** (**⌘,**) → **Plug-ins** → **Terminal** → **Réglages**.

| Réglage | Signification |
|---|---|
| **Shell** | Le shell lancé (`/bin/zsh`, par exemple). |
| **Taille de police** | Taille de la police du terminal. |
| **Historique** | Nombre de lignes que vous pouvez faire défiler vers le haut. |
| **Utiliser Option comme touche Méta** | Option+touche envoie ESC+touche (pour les raccourcis de style Emacs) au lieu de saisir des caractères spéciaux. |
| **La sous-fenêtre suit « cd » du terminal** | Un `cd` dans le terminal change le dossier de la sous-fenêtre. |
| **Afficher les actions sur fichier dans le terminal** | Affiche les actions sur fichier sous forme de commandes shell (voir ci-dessus). |

> [!TIP]
> Sur de nombreux claviers non américains, dont le clavier français, il faut la touche Option pour `@`, `{`, `[` et `|`. Laissez **Utiliser Option comme touche Méta** désactivé, sauf si vous utilisez des raccourcis de style Emacs.

## Clavier dans le terminal {#tastatur}

Tant que le terminal est actif, ces raccourcis s’adressent au terminal et non à Manta : **⌘C**, **⌘V**, **⌘A**, **⌘K**, **⌘←**, **⌘→**, **⌘⌫** et **⌘Z**. Les autres raccourcis de Manta, comme **⇧⌘P**, continuent de fonctionner. Cliquez dans la liste des fichiers pour rendre le clavier à Manta.

## Limites {#grenzen}

- `sh` de busybox (Alpine) et bash 3 ne peuvent pas conserver la saisie entamée lorsque le terminal suit la sous-fenêtre (bash 4 ou ultérieur et zsh le peuvent).
- Dans les conteneurs dont le shell n’est ni bash ni un `sh` POSIX, le terminal ne suit que dans un seul sens.
- Pas de terminal pour S3, FTP et les archives.
