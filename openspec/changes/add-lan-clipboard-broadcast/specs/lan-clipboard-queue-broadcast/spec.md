## ADDED Requirements

### Requirement: Create a LAN queue with password
The system SHALL allow a user to create a LAN queue with a password and start a local host listener for other members to join.

#### Scenario: Create queue successfully
- **WHEN** the user configures a queue name, password, and starts hosting
- **THEN** the system starts listening and provides the host address/port for members

### Requirement: Join a LAN queue with password validation
The system SHALL allow a user to join a LAN queue by providing a host address and password, and SHALL reject the join if the password is invalid.

#### Scenario: Join queue with correct password
- **WHEN** the user enters the correct password for a reachable host
- **THEN** the system joins the queue and shows the member as connected

#### Scenario: Join queue with incorrect password
- **WHEN** the user enters an incorrect password
- **THEN** the system rejects the join and shows an authentication error

### Requirement: Broadcast clipboard items to all members
The system SHALL broadcast newly copied text or image items from any member to all other connected members in the same queue.

#### Scenario: Member copies text
- **WHEN** a connected member copies text
- **THEN** all other members receive the item via the queue broadcast

#### Scenario: Member copies an image
- **WHEN** a connected member copies an image
- **THEN** all other members receive the image via the queue broadcast

### Requirement: Persist received items in history with source info
The system SHALL add received broadcast items to the local clipboard history and SHALL mark the source as a LAN queue item (including sender identity when available).

#### Scenario: Receive broadcast item
- **WHEN** a member receives a broadcast item from the queue
- **THEN** the item is saved to local history with a LAN source marker

### Requirement: Prevent rebroadcast loops
The system SHALL NOT rebroadcast items that were received from the queue and SHALL deduplicate incoming broadcasts using a unique message id.

#### Scenario: Receive and suppress rebroadcast
- **WHEN** a member receives a broadcast item with a known message id
- **THEN** the item is not rebroadcast to the queue

### Requirement: Show queue status and member presence
The system SHALL display the current queue status and the list of connected members, updating when members join or leave.

#### Scenario: Member joins and leaves
- **WHEN** a member connects to or disconnects from the queue
- **THEN** the member list updates in the UI
