# Connexions {#verbindungen}

Manta ouvre les dossiers situés sur des serveurs (SFTP, FTP, FTPS), dans un stockage S3 et dans des conteneurs Docker et Kubernetes comme des dossiers locaux. Vous y copiez, déplacez et renommez exactement comme sur votre Mac.

## Se connecter à un serveur {#verbinden}

1. Appuyez sur **⌘K** (**Se connecter au serveur…**), cliquez sur **Se connecter** dans la barre d’outils, ou cliquez sur le **+** à côté de **Serveurs** dans la barre latérale.
2. Choisissez le **Protocole** en haut : **SFTP**, **FTP**, **FTPS**, **S3** ou **Conteneur**.
3. Remplissez les champs (voir ci-dessous).
4. Facultatif : cliquez sur **Tester la connexion**. Manta indique par exemple « Connecté. Dossier de départ : /home/anna ».
5. Cliquez sur **Se connecter**.

Le serveur s’ouvre dans la sous-fenêtre active (la zone de dialogue indique laquelle). La connexion apparaît sous **Serveurs** dans la barre latérale.

![La zone de dialogue Se connecter au serveur pour SFTP](images/connect-sftp.png)

> [!TIP]
> Vous pouvez tout saisir d’un coup dans le champ **Serveur** : `user@host:port`. Manta répartit les informations dans les champs.

## SFTP {#sftp}

SFTP est la norme sécurisée pour les serveurs Linux et de nombreuses offres d’hébergement.

| Champ | Signification |
|---|---|
| **Serveur**, **Port** | L’adresse du serveur ; port 22, sauf si vous le modifiez. |
| **Utilisateur** | Votre nom d’utilisateur sur le serveur. |
| **S’identifier avec** | **Mot de passe**, **Fichier de clé** (avec une **Phrase de passe** facultative, « uniquement si la clé est chiffrée ») ou **Agent SSH** (« Utilise les clés chargées dans votre ssh-agent en cours d’exécution. »). |
| **Dossier** | Facultatif – par défaut, votre dossier de départ sur le serveur. |

### Vérification de la clé du serveur {#server-schluessel}

> [!WARNING]
> Manta vérifie la clé du serveur à l’aide de votre fichier `~/.ssh/known_hosts` – le même que celui utilisé par `ssh` dans Terminal.
>
> - **Serveur connu, clé correspondante :** Manta se connecte sans rien demander.
> - **Serveur inconnu** (absent de `known_hosts`) : Manta affiche la zone de dialogue **Serveur inconnu** avec le serveur, le type de clé et l’empreinte (`SHA256:…`, telle que l’affiche `ssh-keygen -lf`). Comparez l’empreinte avec celle que vous communique votre administrateur ou votre hébergeur. **Se connecter et mémoriser** ajoute la clé à `known_hosts` (`host`, ou `[host]:port` pour un port autre que 22) et se connecte ; **Annuler** ne se connecte pas.
> - **Clé modifiée :** Manta **ne se connecte pas** et affiche un avertissement rouge (attaque de l’homme du milieu possible). Il n’y a volontairement aucun bouton « se connecter quand même ». Cela se produit aussi lorsqu’un serveur est réinstallé – demandez l’empreinte correcte à l’administrateur. Si elle correspond, supprimez l’ancienne entrée dans Terminal avec la commande indiquée dans la zone de dialogue (comme `ssh-keygen -R host`, **Copier la commande**) et reconnectez-vous.
>
> Les transferts qui se reconnectent en arrière-plan après un redémarrage ne posent jamais de question : avec une clé inconnue, ils attendent **Reconnecter** ; avec une clé modifiée, ils échouent.

## FTP et FTPS {#ftp}

**FTP** transmet tout sans chiffrement, y compris votre mot de passe. Ne l’utilisez que sur votre réseau local ou pour des serveurs publics. **Anonyme** se connecte sans mot de passe, si le serveur le permet.

**FTPS** est du FTP chiffré (TLS explicite, généralement sur le port 21). Manta ne prend pas en charge le FTPS implicite sur le port 990.

### Le canal de données FTPS {#datenkanal}

FTP utilise deux connexions : l’une pour l’identification et les commandes, l’autre (le « canal de données ») pour les listes de dossiers et les fichiers. Certains pare-feu d’entreprise bloquent les connexions de données chiffrées. C’est pourquoi vous pouvez choisir un **Canal de données** :

| Réglage | Comportement |
|---|---|
| **Automatique** (par défaut) | Chiffré. Si le réseau bloque les connexions de données chiffrées, les transferts se poursuivent sans chiffrement ; la connexion reste chiffrée. |
| **Obligatoire** | Toujours chiffré. Échoue si le réseau bloque les connexions de données chiffrées. |
| **Désactivé** | Les listes et les fichiers transitent sans chiffrement ; la connexion reste chiffrée. |

Lorsque **Automatique** se rabat sur un canal de données non chiffré, vous le remarquez :

- La pastille d’emplacement de la barre du chemin d’accès affiche un bouclier d’avertissement.
- Un message unique indique « Connexion de données non chiffrée (le réseau bloque TLS) – la connexion reste chiffrée ».

> [!WARNING]
> En mode de repli, **le contenu et le nom des fichiers transitent sans chiffrement** ; seuls le nom d’utilisateur et le mot de passe restent protégés. Pour des données confidentielles, réglez le canal de données sur **Obligatoire** et utilisez un autre réseau – ou SFTP.

Le réglage est enregistré avec le favori. Voir aussi [Dépannage : canal de données FTP](19-fehlerbehebung.md#ftp-datenkanal).

![Zone de dialogue FTPS avec le choix du canal de données](images/connect-ftps-data-channel.png)

## S3 et stockage compatible S3 {#s3}

**S3** se connecte à Amazon S3 et aux services compatibles. Les buckets et les préfixes apparaissent sous forme de dossiers.

| Champ | Signification |
|---|---|
| **Fournisseur** | Amazon S3, Cloudflare R2, MinIO, Backblaze B2, Wasabi, DigitalOcean Spaces, Hetzner Object Storage ou **Autre service compatible S3**. Le préréglage remplit le point de terminaison et la région. |
| **ID de compte** | Cloudflare R2 uniquement (tableau de bord Cloudflare › R2 › Account ID). |
| **Région**, **Point de terminaison** | Le point de terminaison suit généralement la région. Si un bucket se trouve dans une autre région, Manta corrige lui-même. |
| **S’identifier avec** | **Profil AWS** (clés issues de `~/.aws/credentials`), **Clé d’accès** (ID de clé d’accès et clé secrète) ou **Public** (pour les buckets publics). |
| **Bucket** | Facultatif – sinon vous voyez tous les buckets. |
| **Adressage par chemin** | Nécessaire pour MinIO et la plupart des serveurs auto-hébergés. |

![Zone de dialogue S3 avec un préréglage de fournisseur](images/connect-s3.png)

Bon à savoir :

- Renommer et déplacer au sein d’un bucket revient en coulisses à « copier puis supprimer ». Les objets de plus de 5 Go ne peuvent pas être renommés.
- Les dates de modification et les autorisations ne peuvent pas être définies dans S3.
- La suppression du dernier fichier d’un « dossier » fait disparaître ce dossier s’il n’a pas son propre marqueur de dossier.
- Les profils AWS utilisant SSO, `credential_process` ou assume-role ne fonctionnent pas ; seules les clés statiques fonctionnent.
- Si l’horloge de votre Mac est décalée de plus de 15 minutes, S3 rejette les requêtes (« L’horloge de votre Mac est déréglée »).

## Docker et Kubernetes {#container}

Manta ouvre les fichiers **à l’intérieur des conteneurs** comme un dossier. Il utilise les commandes `docker` et `kubectl`, qui doivent être installées sur votre Mac.

### Depuis la barre latérale

Le plus rapide : cliquez sur un conteneur dans la section **Conteneurs** de la barre latérale. Il s’ouvre dans son répertoire de travail.

### Depuis la zone de dialogue

1. Appuyez sur **⌘K** et choisissez **Conteneur**.
2. Choisissez le **Moteur** : **Docker** ou **Kubernetes**.
3. Docker : choisissez le conteneur dans la liste ou tapez son nom ou son ID. Kubernetes : choisissez le **Contexte**, l’**Espace de noms**, le **Pod** et, si nécessaire, le conteneur.
4. Cliquez sur **Se connecter**.

Les favoris enregistrent le conteneur **par son nom** : un favori continue donc de fonctionner après la recréation du conteneur.

![Sélecteur de conteneur dans la zone de dialogue de connexion](images/connect-container.png)

### Conteneurs arrêtés et images sans shell

- Les **conteneurs arrêtés** s’ouvrent en **lecture seule** (pastille **Arrêté**). Vous pouvez en copier des fichiers vers l’extérieur ; démarrez le conteneur pour apporter des modifications.
- Les conteneurs **sans shell** (images distroless ou scratch) sont également en lecture seule.
- Manta lit un dossier d’un conteneur arrêté d’un seul bloc ; s’il dépasse 512 Mo, il indique « trop volumineux ». Démarrez alors le conteneur.

### Un shell dans le conteneur

Clic droit → **Ouvrir un shell dans le conteneur** (également dans la palette de commandes) ouvre un shell à l’intérieur du conteneur dans le terminal situé sous la sous-fenêtre, directement dans le dossier que vous consultez. Voir [Terminal](11-terminal.md#container-ssh).

> [!NOTE]
> Manta ne modifie que les fichiers sur lesquels vous agissez vous-même. Il ne démarre, n’arrête et ne supprime jamais de conteneurs.

## Les connexions en favoris {#favoriten}

1. Activez **Ajouter aux Favoris** dans la zone de dialogue de connexion.
2. Saisissez éventuellement un nom pour la barre latérale.
3. Facultatif : **Mémoriser le mot de passe** (ou **Mémoriser la phrase de passe**, **Mémoriser la clé secrète**).

> [!NOTE]
> Les mots de passe, phrases de passe et clés secrètes S3 mémorisés sont stockés dans le **trousseau macOS** (trousseau de session, élément « com.manta-filemanager.manta.server », compte tel que `sftp://user@host:22`), et non dans `favorites.json`. Le fichier note seulement qu’un élément existe. Manta lit le mot de passe lorsque vous vous connectez ; macOS peut demander une fois si Manta est autorisé à utiliser le trousseau.
>
> Les anciennes versions stockaient les mots de passe mémorisés sans chiffrement dans `favorites.json`. Au premier lancement, Manta les déplace dans le trousseau et réécrit le fichier sans eux. En cas d’échec (trousseau verrouillé ou accès refusé), le fichier reste inchangé et Manta réessaie au lancement suivant.
>
> La suppression d’un favori supprime aussi son élément du trousseau (sauf si un autre favori pour le même compte sur le même serveur l’utilise encore). Lorsque vous modifiez un favori, le champ du mot de passe indique « enregistré dans le trousseau » ; laissez-le vide pour conserver le mot de passe enregistré.

Pour modifier un favori, clic droit → **Modifier la connexion…** (la zone de dialogue « Modifier la connexion »).

## Se déconnecter {#trennen}

Survolez la connexion sous **Serveurs** dans la barre latérale et cliquez sur l’icône d’éjection (**Déconnecter**). Les sous-fenêtres qui affichaient le serveur indiquent « Déconnecté ».

## Quand ça ne fonctionne pas {#fehler}

Manta transforme les erreurs techniques en messages compréhensibles accompagnés d’un conseil. Les plus fréquents :

| Message | Ce que vous pouvez faire |
|---|---|
| « Impossible de trouver « host » » | Vérifiez que l’adresse du serveur ne contient pas de faute de frappe. |
| « host a refusé la connexion sur le port … » | Le port et le protocole sont-ils corrects ? Le serveur fonctionne-t-il ? |
| « host ne répond pas » | Vérifiez votre réseau, votre VPN et votre pare-feu. |
| « Le serveur n’a pas accepté ces identifiants » | Vérifiez le nom d’utilisateur et le mot de passe, ou essayez une autre méthode d’identification. |
| « Aucune clé utilisable dans ssh-agent » | Ajoutez une clé avec `ssh-add`. |
| « Impossible d’établir une connexion sécurisée » | Le serveur ne prend pas en charge FTPS sur ce port, ou son certificat n’est pas approuvé. |
| « Docker n’est pas en cours d’exécution » | Lancez Docker Desktop. |
| « kubectl n’est pas installé » | `brew install kubectl` |

Plus d’aide : [Dépannage](19-fehlerbehebung.md).

## Limites {#grenzen}

- Pas de FTPS implicite (port 990).
- Kubernetes : pas de redirection de port, pas de conteneurs de débogage éphémères ; les pods ont besoin de `sh`.
- L’écriture dans les conteneurs passe par `cat` ; les nouveaux fichiers appartiennent à l’utilisateur sous lequel `exec` s’exécute.
- Ce n’est pas ici que vous connectez Google Drive et consorts, mais via les dossiers cloud de votre Mac, voir [Stockage cloud](09-cloud.md). Manta n’intègre pas sa propre connexion Google ; ce n’est que si vous saisissez votre **propre** client OAuth dans les Réglages que « Google Drive » apparaît aussi ici, voir [Google Drive pour les experts](09-cloud.md#experte).
