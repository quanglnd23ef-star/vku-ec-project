#![no_std]
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

// Các mã lỗi của contract (mỗi lỗi một số, bắt đầu từ 1)
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    InvalidAmount = 4,
    AlreadyAdded = 5,
    NotMember = 6,
    AlreadyPaid = 7,
    BillClosed = 8,
    UnpaidRemaining = 9,
    NoMembers = 10,
    Overflow = 11,
}

// Thông tin đợt chia tiền
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bill {
    pub admin: Address,
    pub share_amount: i128,
    pub total_members: u32,
    pub paid_members: u32,
    pub open: bool,
    pub created_at: u64,
}

// Trạng thái của từng thành viên
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub listed: bool,
    pub paid: bool,
    pub paid_at: u64,
}

// Khoá lưu trữ dữ liệu
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Bill,
    Member(Address),
}

const TTL_THRESHOLD: u32 = 100_000;
const TTL_EXTEND_TO: u32 = 400_000;

// Đọc thông tin hóa đơn, lỗi nếu chưa khởi tạo
fn load_bill(env: &Env) -> Result<Bill, Error> {
    env.storage()
        .persistent()
        .get::<DataKey, Bill>(&DataKey::Bill)
        .ok_or(Error::NotInitialized)
}

// Lưu thông tin hóa đơn và gia hạn thời gian sống của dữ liệu
fn save_bill(env: &Env, bill: &Bill) {
    let key = DataKey::Bill;
    env.storage().persistent().set(&key, bill);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

// Đọc trạng thái thành viên, chưa có trong danh sách thì trả về toàn false
fn load_member(env: &Env, member: &Address) -> Member {
    env.storage()
        .persistent()
        .get::<DataKey, Member>(&DataKey::Member(member.clone()))
        .unwrap_or(Member {
            listed: false,
            paid: false,
            paid_at: 0,
        })
}

// Lưu trạng thái thành viên và gia hạn thời gian sống của dữ liệu
fn save_member(env: &Env, member: &Address, data: &Member) {
    let key = DataKey::Member(member.clone());
    env.storage().persistent().set(&key, data);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

// Kiểm tra chữ ký, quyền người trả hóa đơn và đợt chia tiền còn đang mở
fn check_admin(env: &Env, admin: &Address) -> Result<Bill, Error> {
    let bill = load_bill(env)?;
    admin.require_auth();
    if bill.admin != *admin {
        return Err(Error::NotAdmin);
    }
    if !bill.open {
        return Err(Error::BillClosed);
    }
    Ok(bill)
}

#[contract]
pub struct SplitBill;

#[contractimpl]
impl SplitBill {
    // Khởi tạo đợt chia tiền một lần duy nhất với người trả hóa đơn và số tiền mỗi phần
    pub fn init(env: Env, admin: Address, share_amount: i128) -> Result<(), Error> {
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Bill) {
            return Err(Error::AlreadyInitialized);
        }
        if share_amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let bill = Bill {
            admin,
            share_amount,
            total_members: 0,
            paid_members: 0,
            open: true,
            created_at: env.ledger().timestamp(),
        };
        save_bill(&env, &bill);
        Ok(())
    }

    // Người trả hóa đơn thêm một thành viên vào danh sách, mỗi người chỉ thêm một lần
    pub fn add_member(env: Env, admin: Address, member: Address) -> Result<u32, Error> {
        let mut bill = check_admin(&env, &admin)?;
        let mut data = load_member(&env, &member);
        if data.listed {
            return Err(Error::AlreadyAdded);
        }
        data.listed = true;
        save_member(&env, &member, &data);
        bill.total_members = bill.total_members.checked_add(1).ok_or(Error::Overflow)?;
        save_bill(&env, &bill);
        Ok(bill.total_members)
    }

    // Người trả hóa đơn xác nhận thành viên đã hoàn tiền, mỗi người chỉ xác nhận một lần
    pub fn confirm_paid(env: Env, admin: Address, member: Address) -> Result<u32, Error> {
        let mut bill = check_admin(&env, &admin)?;
        let mut data = load_member(&env, &member);
        if !data.listed {
            return Err(Error::NotMember);
        }
        if data.paid {
            return Err(Error::AlreadyPaid);
        }
        data.paid = true;
        data.paid_at = env.ledger().timestamp();
        save_member(&env, &member, &data);
        bill.paid_members = bill.paid_members.checked_add(1).ok_or(Error::Overflow)?;
        save_bill(&env, &bill);
        Ok(bill.paid_members)
    }

    // Người trả hóa đơn chốt sổ khi tất cả thành viên đã thanh toán xong
    pub fn close_bill(env: Env, admin: Address) -> Result<(), Error> {
        let mut bill = check_admin(&env, &admin)?;
        if bill.total_members == 0 {
            return Err(Error::NoMembers);
        }
        if bill.paid_members < bill.total_members {
            return Err(Error::UnpaidRemaining);
        }
        bill.open = false;
        save_bill(&env, &bill);
        Ok(())
    }

    // Ai cũng xem được thông tin đợt chia tiền: số tiền mỗi phần, số người, số người đã trả, trạng thái
    pub fn get_bill(env: Env) -> Result<Bill, Error> {
        load_bill(&env)
    }

    // Ai cũng xem được thành viên có trong danh sách không và đã trả chưa
    pub fn get_member(env: Env, member: Address) -> Member {
        load_member(&env, &member)
    }
}