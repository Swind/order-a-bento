use crate::identity::OrganizerId;
use crate::shared::{
    DomainError, Id, RosterName,
    text::{ADDRESS_MAX_LEN, optional_text},
};

pub struct DeliveryAddressMarker;
pub type DeliveryAddressId = Id<DeliveryAddressMarker>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryAddress {
    pub id: DeliveryAddressId,
    pub organizer_id: OrganizerId,
    pub name: String,
    pub address: String,
}

impl DeliveryAddress {
    pub fn new(organizer_id: OrganizerId, name: &str, address: &str) -> Result<Self, DomainError> {
        let name = RosterName::new(name)?.to_string();
        let address = optional_text("address", Some(address), ADDRESS_MAX_LEN)?
            .ok_or_else(|| DomainError::invalid("address", "請輸入送餐地址"))?;
        Ok(Self {
            id: DeliveryAddressId::new(),
            organizer_id,
            name,
            address,
        })
    }

    pub fn snapshot(&self) -> String {
        format!("{}（{}）", self.name, self.address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_delivery_address() {
        let org = OrganizerId::new();
        let a = DeliveryAddress::new(org, " 教室 A ", " 台北市中正區 3 樓 ").unwrap();
        assert_eq!(a.snapshot(), "教室 A（台北市中正區 3 樓）");
        assert!(DeliveryAddress::new(org, " ", "地址").is_err());
        assert!(DeliveryAddress::new(org, "教室", " ").is_err());
        assert!(DeliveryAddress::new(org, "教室", &"字".repeat(201)).is_err());
    }
}
