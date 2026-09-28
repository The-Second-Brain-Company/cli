use usage::{Args, Cli, Subcommands};

#[derive(Cli)]
#[usage(bin = "brain", version = env!("CARGO_PKG_VERSION"), about = "Second Brain for people and agents. Results are JSON; help and completions come from usage.", completion)]
pub struct Brain {
    #[usage(long, global, help = "Service origin; overrides BRAIN_ORIGIN")]
    pub origin: Option<String>,
    #[usage(
        long,
        global,
        help = "Project directory containing .brain/config.toml (default: current directory)"
    )]
    pub project: Option<String>,
    #[usage(
        long,
        global,
        help = "Explicit Brain ID for this invocation; does not change project selection"
    )]
    pub brain: Option<String>,
    #[usage(long, global, help = "Indent JSON output")]
    pub pretty: bool,
    #[usage(subcommand)]
    pub command: Command,
}

#[derive(Subcommands)]
pub enum Command {
    #[usage(
        help = "Sign in through the browser and select your only Brain for an unconfigured project"
    )]
    Login(Login),
    #[usage(help = "Revoke this CLI connection and delete its local credentials")]
    Logout,
    #[usage(help = "Inspect the signed-in account and manage your own profile or apps")]
    Account(Account),
    #[usage(help = "List or create Brains")]
    Brains(Brains),
    #[usage(help = "Verify and select a Brain for this project")]
    Use(Select),
    #[usage(help = "Show local selection without accessing knowledge")]
    Config,
    #[usage(help = "Verify the selected Brain, identity, role, and scopes")]
    Whoami,
    #[usage(help = "Read repository setup and active runs")]
    Status,
    #[usage(help = "Search selected Brain knowledge")]
    Search(Search),
    #[usage(help = "Read a knowledge file with bounded line ranges")]
    Read(Read),
    #[usage(help = "Record supplied facts with a durable retry ID")]
    Record(Record),
    #[usage(help = "Browse, search, record, and edit knowledge")]
    Knowledge(Knowledge),
    #[usage(help = "Inspect or cancel recording runs")]
    Runs(Runs),
    #[usage(help = "List people, invite members, and manage roles")]
    People(People),
    #[usage(help = "Recover repository setup and manage read-only Git credentials")]
    Repository(Repository),
    #[usage(help = "Inspect or disconnect your apps in the selected Brain")]
    Connections(Connections),
    #[usage(help = "Generate a retry ID before a create or record operation")]
    RequestId,
    #[usage(help = "Print a shell completion script")]
    Completions(Completions),
}

#[derive(Args)]
pub struct Completions {
    #[usage(choices("bash", "zsh", "fish"))]
    pub shell: String,
}

#[derive(Args)]
pub struct Login {
    #[usage(
        long,
        help = "Skip automatic project selection when the account has exactly one Brain"
    )]
    pub no_select: bool,
    #[usage(
        long,
        help = "Print the authorization URL without opening a browser; keep this command running"
    )]
    pub no_browser: bool,
    #[usage(
        long,
        default = "600",
        help = "Seconds to wait for browser consent (1-1800)"
    )]
    pub timeout: u64,
    #[usage(
        long,
        default = "knowledge:read brains:access knowledge:write organization:manage connections:manage account:manage brains:create",
        help = "Space-separated permissions to request in browser consent"
    )]
    pub scopes: String,
}

#[derive(Args)]
pub struct Account {
    #[usage(subcommand)]
    pub command: AccountCommand,
}
#[derive(Subcommands)]
pub enum AccountCommand {
    Show,
    Profile(Name),
    Disconnect(ConnectionId),
}
#[derive(Args)]
pub struct Name {
    #[usage(help = "Name; an empty string clears or skips the personal name")]
    pub name: String,
}
#[derive(Args)]
pub struct Brains {
    #[usage(subcommand)]
    pub command: BrainCommand,
}
#[derive(Subcommands)]
pub enum BrainCommand {
    List,
    Create(Create),
}
#[derive(Args)]
pub struct Create {
    pub name: String,
    #[usage(
        long,
        help = "Required durable retry ID; reuse with exactly the same name and original Brain"
    )]
    pub request_id: String,
}
#[derive(Args)]
pub struct Select {
    pub brain_id: String,
}
#[derive(Args)]
pub struct Knowledge {
    #[usage(subcommand)]
    pub command: KnowledgeCommand,
}
#[derive(Subcommands)]
pub enum KnowledgeCommand {
    Show,
    Document(Document),
    List(Revision),
    Revision,
    Search(Search),
    Grep(Grep),
    Read(Read),
    ReadMany(ReadMany),
    Record(Record),
    Replace(Replace),
    Attachment(Attachment),
}
#[derive(Args)]
pub struct Document {
    pub path: String,
    #[usage(long)]
    pub revision: Option<String>,
}
#[derive(Args)]
pub struct Revision {
    #[usage(long)]
    pub revision: Option<String>,
}
#[derive(Args)]
pub struct Search {
    pub query: String,
    #[usage(long)]
    pub path: Option<String>,
    #[usage(long, default = "0")]
    pub offset: u64,
    #[usage(long, default = "20")]
    pub limit: u64,
    #[usage(long)]
    pub revision: Option<String>,
}
#[derive(Args)]
pub struct Grep {
    pub pattern: String,
    #[usage(long)]
    pub path: Option<String>,
    #[usage(long)]
    pub case_sensitive: bool,
    #[usage(long, default = "0")]
    pub offset: u64,
    #[usage(long, default = "50")]
    pub limit: u64,
    #[usage(long)]
    pub revision: Option<String>,
}
#[derive(Args)]
pub struct Read {
    pub path: String,
    #[usage(long, default = "1")]
    pub start_line: u64,
    #[usage(long, default = "200")]
    pub limit: u64,
    #[usage(long)]
    pub revision: Option<String>,
}
#[derive(Args)]
pub struct ReadMany {
    #[usage(
        long,
        help = "JSON array of {path, startLine?, limit?}; use - for stdin"
    )]
    pub file: String,
    #[usage(long)]
    pub revision: Option<String>,
}
#[derive(Args)]
pub struct Record {
    #[usage(long, help = "Facts to save; use --file for larger content")]
    pub text: Option<String>,
    #[usage(long, help = "UTF-8 facts file; use - for stdin")]
    pub file: Option<String>,
    #[usage(
        long,
        help = "Required durable retry ID; preserve input and Brain on retries"
    )]
    pub request_id: String,
    #[usage(long, help = "Local attachment; repeat up to five times")]
    pub attachment: Vec<String>,
    #[usage(
        long,
        default = "25",
        help = "Seconds to wait for the initial durable result (0-25)"
    )]
    pub wait: u64,
}
#[derive(Args)]
pub struct Replace {
    #[usage(
        long,
        help = "JSON object with content, baseRevision, optional documents and summary; - for stdin"
    )]
    pub file: String,
}
#[derive(Args)]
pub struct Attachment {
    pub path: String,
    #[usage(long)]
    pub revision: Option<String>,
    #[usage(long, default = "1")]
    pub start_line: u64,
    #[usage(long, default = "200")]
    pub limit: u64,
}
#[derive(Args)]
pub struct Runs {
    #[usage(subcommand)]
    pub command: RunCommand,
}
#[derive(Subcommands)]
pub enum RunCommand {
    Get(Run),
    Cancel(RunId),
}
#[derive(Args)]
pub struct Run {
    pub run_id: String,
    #[usage(long, default = "25", help = "Long poll for 0-25 seconds")]
    pub wait: u64,
}
#[derive(Args)]
pub struct RunId {
    pub run_id: String,
}
#[derive(Args)]
pub struct People {
    #[usage(subcommand)]
    pub command: PeopleCommand,
}
#[derive(Subcommands)]
pub enum PeopleCommand {
    List,
    Invite(Invite),
    Revoke(InvitationId),
    Role(ChangeRole),
    Transfer(Transfer),
}
#[derive(Args)]
pub struct Invite {
    pub email: String,
    #[usage(long, choices("read", "write", "admin"), default = "read")]
    pub role: String,
    #[usage(
        long,
        help = "Write a local development invitation link to a new private file"
    )]
    pub secret_file: Option<String>,
}
#[derive(Args)]
pub struct InvitationId {
    pub invitation_id: String,
}
#[derive(Args)]
pub struct ChangeRole {
    pub user_id: String,
    #[usage(long, choices("read", "write", "admin"))]
    pub role: String,
}
#[derive(Args)]
pub struct Transfer {
    pub user_id: String,
    #[usage(long, help = "Current Owner ID from people list")]
    pub expected_owner: String,
}
#[derive(Args)]
pub struct Repository {
    #[usage(subcommand)]
    pub command: RepositoryCommand,
}
#[derive(Subcommands)]
pub enum RepositoryCommand {
    Retry,
    Access(Access),
}
#[derive(Args)]
pub struct Access {
    #[usage(subcommand)]
    pub command: AccessCommand,
}
#[derive(Subcommands)]
pub enum AccessCommand {
    List,
    Create(AccessCreate),
    Revoke(AccessId),
}
#[derive(Args)]
pub struct AccessCreate {
    pub name: String,
    #[usage(
        long,
        help = "Write the one-time credential to a new private file; never stdout"
    )]
    pub secret_file: String,
}
#[derive(Args)]
pub struct AccessId {
    pub access_id: String,
}
#[derive(Args)]
pub struct Connections {
    #[usage(subcommand)]
    pub command: ConnectionCommand,
}
#[derive(Subcommands)]
pub enum ConnectionCommand {
    List,
    Disconnect(ConnectionId),
}
#[derive(Args)]
pub struct ConnectionId {
    pub connection_id: String,
}
